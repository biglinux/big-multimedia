//! Relm4 podcast tab — subscriptions list, RSS search/discovery, add-feed
//! dialog.
//!
//! Rendering: ONE virtualized [`MediaListView`](crate::media_list) carries both
//! sections (Subscriptions + Search Results) as a single recycling `ListView`,
//! so only the ~visible rows become widgets and only their artwork is fetched
//! (in `bind`, off-thread, cached). Disk reads/writes (subscriptions + the
//! title cache) run off the GTK thread; missing feed titles resolve once in the
//! background and re-render, instead of one fetch per row at build time.
//!
//! Push-model: subscription CRUD, podcast search (network) and feed-title
//! fetches are internal; selecting a feed emits `PodcastPanelOutput::LoadFeed`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use relm4::{Component, ComponentParts, ComponentSender};

use crate::media_list::{MediaListView, MediaRow, RowImage};
use big_media_data::i18n::i18n;
use big_media_data::library::podcast::{
    PodcastSearchResult, cache_feed_title, fetch_feed, load_subscriptions, load_title_cache,
    save_subscriptions, search_podcasts,
};
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};

/// Artwork pixel size for a podcast row.
const ICON_PX: i32 = 40;
/// Row tag: a subscription (unsubscribe action).
const TAG_SUB: u32 = 1;
/// Row tag: a search result (subscribe action).
const TAG_SEARCH: u32 = 2;

/// Messages accepted by [`PodcastPanel`].
#[derive(Debug)]
pub enum PodcastPanelInput {
    /// (Re)load the subscription list + title cache from disk (off-thread).
    RefreshFeeds,
    /// Disk load finished: subscriptions + cached titles.
    FeedsLoaded {
        /// Subscribed feed URLs.
        subs: Vec<String>,
        /// Cached feed-url → title map.
        titles: HashMap<String, String>,
    },
    /// Select a feed: highlight it and ask the shell to load it.
    ActivateFeed(String),
    /// Run a podcast search for `query`.
    Search(String),
    /// Render fetched search results.
    ShowSearchResults(Vec<PodcastSearchResult>),
    /// Search request failed (network).
    SearchFailed,
    /// Hide the search section (query cleared).
    ClearSearch,
    /// Subscribe to a feed URL (save off-thread + refresh + toast).
    Subscribe(String),
    /// Unsubscribe from a feed URL (save off-thread + refresh).
    Unsubscribe(String),
    /// Feed-title fetch finished for a subscribed URL.
    FeedTitleResolved {
        /// Feed URL.
        url: String,
        /// Resolved title, or `None` when the feed could not be fetched.
        title: Option<String>,
    },
    /// Open the "add RSS feed" dialog.
    AddFeedDialog,
}

/// Intents emitted by the podcast panel.
#[derive(Debug)]
pub enum PodcastPanelOutput {
    /// Load this feed (shell switches to podcast mode and rebuilds the queue).
    LoadFeed(String),
    /// Show a transient notification toast.
    Notify(String),
}

/// Relm4 podcast tab component.
pub struct PodcastPanel {
    list: MediaListView,
    scroll: gtk::ScrolledWindow,
    search_spinner: gtk::Spinner,
    status: adw::StatusPage,
    subs: Vec<String>,
    titles: HashMap<String, String>,
    search_results: Vec<PodcastSearchResult>,
    /// Subscribed URLs, shared into the row-suffix builder so the search
    /// "subscribe" button reflects the current set at bind time.
    subscribed: Rc<RefCell<HashSet<String>>>,
    pending_title_fetches: HashSet<String>,
    failed_title_fetches: HashSet<String>,
    active_feed: String,
}

impl Component for PodcastPanel {
    type Init = ();
    type Input = PodcastPanelInput;
    type Output = PodcastPanelOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let search_entry = gtk::SearchEntry::builder()
            .hexpand(true)
            .placeholder_text(i18n("Search podcasts…"))
            .build();
        search_entry.update_property(&[gtk::accessible::Property::Label(&i18n("Search podcasts"))]);
        let s = sender.clone();
        search_entry.connect_search_changed(move |entry| {
            if entry.text().trim().is_empty() {
                s.input(PodcastPanelInput::ClearSearch);
            }
        });
        let s = sender.clone();
        search_entry.connect_activate(move |entry| {
            let query = entry.text().trim().to_string();
            if query.is_empty() {
                s.input(PodcastPanelInput::ClearSearch);
            } else {
                s.input(PodcastPanelInput::Search(query));
            }
        });

        let add_button = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .css_classes(["flat"])
            .build();
        big_relm4_components::feedback::tooltip::set(&add_button, i18n("Add RSS feed"));
        add_button.update_property(&[gtk::accessible::Property::Label(&i18n("Add RSS feed"))]);
        let s = sender.clone();
        add_button.connect_clicked(move |_| s.input(PodcastPanelInput::AddFeedDialog));

        let toolbar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        toolbar.append(&search_entry);
        toolbar.append(&add_button);
        root.append(&toolbar);

        let search_spinner = gtk::Spinner::builder()
            .spinning(false)
            .visible(false)
            .margin_top(24)
            .margin_bottom(24)
            .width_request(32)
            .height_request(32)
            .halign(gtk::Align::Center)
            .build();
        root.append(&search_spinner);

        let status = adw::StatusPage::builder()
            .icon_name("microphone-sensitivity-high-symbolic")
            .title(i18n("No Podcasts"))
            .description(i18n("Search for podcasts or add an RSS feed"))
            .vexpand(true)
            .build();
        status.add_css_class("compact");
        root.append(&status);

        let subscribed: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
        let list = MediaListView::new(
            {
                let sender = sender.clone();
                move |url| sender.input(PodcastPanelInput::ActivateFeed(url.to_owned()))
            },
            row_suffix_builder(&sender, &subscribed),
        );
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list.widget)
            .visible(false)
            .build();
        root.append(&scroll);

        sender.input(PodcastPanelInput::RefreshFeeds);

        ComponentParts {
            model: Self {
                list,
                scroll,
                search_spinner,
                status,
                subs: Vec::new(),
                titles: HashMap::new(),
                search_results: Vec::new(),
                subscribed,
                pending_title_fetches: HashSet::new(),
                failed_title_fetches: HashSet::new(),
                active_feed: String::new(),
            },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match message {
            PodcastPanelInput::RefreshFeeds => {
                // Disk read off the GTK thread.
                let s = sender.clone();
                glib::spawn_future_local(async move {
                    let loaded = gio::spawn_blocking(|| (load_subscriptions(), load_title_cache()))
                        .await
                        .unwrap_or_default();
                    s.input(PodcastPanelInput::FeedsLoaded {
                        subs: loaded.0,
                        titles: loaded.1,
                    });
                });
            }
            PodcastPanelInput::FeedsLoaded { subs, titles } => {
                self.subs = subs;
                self.titles = titles;
                self.pending_title_fetches
                    .retain(|url| self.subs.contains(url));
                self.failed_title_fetches
                    .retain(|url| self.subs.contains(url));
                self.sync_subscribed();
                self.rebuild_list();
                self.resolve_missing_titles(&sender);
            }
            PodcastPanelInput::ActivateFeed(url) => {
                self.active_feed.clone_from(&url);
                self.list.set_now_playing(&self.active_feed);
                let _ = sender.output(PodcastPanelOutput::LoadFeed(url));
            }
            PodcastPanelInput::Search(query) => self.start_search(query, &sender),
            PodcastPanelInput::ShowSearchResults(results) => {
                self.search_spinner.set_spinning(false);
                self.search_spinner.set_visible(false);
                self.status.set_visible(false);
                self.search_results = results;
                self.rebuild_list();
            }
            PodcastPanelInput::SearchFailed => {
                self.search_spinner.set_spinning(false);
                self.search_spinner.set_visible(false);
                let _ = sender.output(PodcastPanelOutput::Notify(i18n(
                    "Connection error — try again",
                )));
            }
            PodcastPanelInput::ClearSearch => {
                self.search_spinner.set_visible(false);
                self.search_spinner.set_spinning(false);
                self.search_results.clear();
                self.rebuild_list();
            }
            PodcastPanelInput::Subscribe(url) => {
                if self.subs.contains(&url) {
                    let _ = sender.output(PodcastPanelOutput::Notify(i18n("Already subscribed")));
                } else {
                    self.failed_title_fetches.remove(&url);
                    self.subs.push(url);
                    self.persist_subscriptions(&sender, Some(i18n("Subscribed!")));
                    self.sync_subscribed();
                    self.rebuild_list();
                    self.resolve_missing_titles(&sender);
                }
            }
            PodcastPanelInput::Unsubscribe(url) => {
                self.subs.retain(|u| u != &url);
                self.pending_title_fetches.remove(&url);
                self.failed_title_fetches.remove(&url);
                self.persist_subscriptions(&sender, None);
                self.sync_subscribed();
                self.rebuild_list();
            }
            PodcastPanelInput::FeedTitleResolved { url, title } => {
                self.pending_title_fetches.remove(&url);
                if !is_subscribed_url(&self.subs, &url) {
                    self.rebuild_list();
                    return;
                }
                if let Some(title) = title {
                    self.failed_title_fetches.remove(&url);
                    self.titles.insert(url, title);
                } else {
                    self.failed_title_fetches.insert(url);
                }
                self.rebuild_list();
            }
            PodcastPanelInput::AddFeedDialog => present_add_feed_dialog(root, &sender),
        }
    }
}

impl PodcastPanel {
    fn sync_subscribed(&self) {
        let mut set = self.subscribed.borrow_mut();
        set.clear();
        set.extend(self.subs.iter().cloned());
    }

    /// Persist the current subscription list off the GTK thread; optional toast.
    fn persist_subscriptions(&self, sender: &ComponentSender<Self>, toast: Option<String>) {
        let subs = self.subs.clone();
        let s = sender.clone();
        glib::spawn_future_local(async move {
            let saved = gio::spawn_blocking(move || save_subscriptions(&subs)).await;
            if let Ok(Err(error)) = saved {
                log::warn!("Failed to save podcast subscriptions: {error}");
            }
            if let Some(message) = toast {
                let _ = s.output(PodcastPanelOutput::Notify(message));
            }
        });
    }

    /// Start an async podcast search, toggling the spinner.
    fn start_search(&self, query: String, sender: &ComponentSender<Self>) {
        self.status.set_visible(false);
        self.search_spinner.set_visible(true);
        self.search_spinner.set_spinning(true);

        let s = sender.clone();
        glib::spawn_future_local(async move {
            let q = query.clone();
            let results = gio::spawn_blocking(move || search_podcasts(&q))
                .await
                .unwrap_or(None);
            match results {
                Some(r) => s.input(PodcastPanelInput::ShowSearchResults(r)),
                None => s.input(PodcastPanelInput::SearchFailed),
            }
        });
    }

    /// Build the combined section list (Subscriptions + Search Results) and swap
    /// it into the view; toggle the empty status page.
    fn rebuild_list(&self) {
        let mut rows: Vec<MediaRow> = Vec::new();

        if !self.subs.is_empty() {
            rows.push(MediaRow::header(i18n("Subscriptions")));
            for url in &self.subs {
                let title = self.titles.get(url).cloned();
                let has_title = title.is_some();
                rows.push(MediaRow {
                    id: url.clone(),
                    title: title.clone().unwrap_or_else(|| url.clone()),
                    subtitle: subscription_title_subtitle(
                        has_title,
                        self.failed_title_fetches.contains(url),
                    ),
                    image: RowImage::Initials {
                        text: title.unwrap_or_else(|| url.clone()),
                        size: ICON_PX,
                    },
                    header: None,
                    favicon: String::new(),
                    group: String::new(),
                    tag: TAG_SUB,
                });
            }
        }

        if !self.search_results.is_empty() {
            rows.push(MediaRow::header(i18n("Search Results")));
            for result in &self.search_results {
                rows.push(search_row(result));
            }
        }

        let has_content = !rows.is_empty();
        self.list.set_items(rows);
        self.list.set_now_playing(&self.active_feed);
        self.scroll.set_visible(has_content);
        self.status
            .set_visible(!has_content && !self.search_spinner.is_visible());
    }

    /// Resolve titles for subscriptions missing from the cache, off-thread and
    /// ONCE (not one fetch per row at build): each fetch caches its title and
    /// re-renders. Bounded by the number of uncached subs (the user's own list).
    fn resolve_missing_titles(&mut self, sender: &ComponentSender<Self>) {
        let missing = title_fetch_candidates(
            &self.subs,
            &self.titles,
            &self.pending_title_fetches,
            &self.failed_title_fetches,
        );
        for url in missing {
            self.pending_title_fetches.insert(url.clone());
            let s = sender.clone();
            glib::spawn_future_local(async move {
                let request_url = url.clone();
                let title = gio::spawn_blocking(move || {
                    let feed = fetch_feed(&request_url)?;
                    cache_feed_title(&request_url, &feed.title);
                    Some(feed.title)
                })
                .await
                .ok()
                .flatten();
                s.input(PodcastPanelInput::FeedTitleResolved { url, title });
            });
        }
    }
}

fn subscription_title_subtitle(has_title: bool, has_failed_title_fetch: bool) -> Option<String> {
    if has_title {
        return None;
    }
    Some(if has_failed_title_fetch {
        i18n("Title unavailable")
    } else {
        i18n("Loading…")
    })
}

fn title_fetch_candidates(
    subs: &[String],
    titles: &HashMap<String, String>,
    pending_title_fetches: &HashSet<String>,
    failed_title_fetches: &HashSet<String>,
) -> Vec<String> {
    subs.iter()
        .filter(|url| !titles.contains_key(*url))
        .filter(|url| !pending_title_fetches.contains(*url))
        .filter(|url| !failed_title_fetches.contains(*url))
        .cloned()
        .collect()
}

fn is_subscribed_url(subs: &[String], url: &str) -> bool {
    subs.iter().any(|sub| sub == url)
}

/// Build a search-result [`MediaRow`] (artwork loads visible-only in `bind`).
fn search_row(result: &PodcastSearchResult) -> MediaRow {
    let image = if result.artwork_url.is_empty() {
        RowImage::Initials {
            text: result.title.clone(),
            size: ICON_PX,
        }
    } else {
        RowImage::Remote {
            url: result.artwork_url.clone(),
            initials: result.title.clone(),
            size: ICON_PX,
            fetch: std::sync::Arc::new(big_os_kit::http_client::http_get_bytes),
        }
    };
    MediaRow {
        id: result.feed_url.clone(),
        title: result.title.clone(),
        subtitle: Some(search_result_subtitle(result)),
        image,
        header: None,
        favicon: String::new(),
        group: String::new(),
        tag: TAG_SEARCH,
    }
}

/// The per-row action-button factory: subscriptions get unsubscribe, search
/// results get subscribe (reflecting the current subscribed set).
fn row_suffix_builder(
    sender: &ComponentSender<PodcastPanel>,
    subscribed: &Rc<RefCell<HashSet<String>>>,
) -> impl Fn(&MediaRow) -> Vec<gtk::Widget> + 'static {
    let sender = sender.clone();
    let subscribed = subscribed.clone();
    move |row| match row.tag {
        TAG_SUB => {
            let remove = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .valign(gtk::Align::Center)
                .css_classes(["flat", "error"])
                .build();
            big_relm4_components::feedback::tooltip::set(&remove, i18n("Unsubscribe"));
            remove.update_property(&[gtk::accessible::Property::Label(&i18n(
                "Unsubscribe from podcast",
            ))]);
            let s = sender.clone();
            let url = row.id.clone();
            remove.connect_clicked(move |_| s.input(PodcastPanelInput::Unsubscribe(url.clone())));
            vec![remove.upcast()]
        }
        TAG_SEARCH => {
            let already = subscribed.borrow().contains(&row.id);
            let sub = gtk::Button::builder()
                .icon_name(if already {
                    "object-select-symbolic"
                } else {
                    "list-add-symbolic"
                })
                .valign(gtk::Align::Center)
                .sensitive(!already)
                .css_classes(["flat"])
                .build();
            big_relm4_components::feedback::tooltip::set(&sub, i18n("Subscribe"));
            sub.update_property(&[gtk::accessible::Property::Label(&i18n(
                "Subscribe to podcast",
            ))]);
            if !already {
                let s = sender.clone();
                let feed_url = row.id.clone();
                sub.connect_clicked(move |button| {
                    s.input(PodcastPanelInput::Subscribe(feed_url.clone()));
                    button.set_sensitive(false);
                    button.set_icon_name("object-select-symbolic");
                });
            }
            vec![sub.upcast()]
        }
        _ => Vec::new(),
    }
}

/// Format the "author · genre · N episodes" subtitle for a search result.
fn search_result_subtitle(result: &PodcastSearchResult) -> String {
    let episodes = i18n("episodes");
    if !result.author.is_empty() && !result.genre.is_empty() {
        format!(
            "{} · {} · {} {episodes}",
            result.author, result.genre, result.episode_count
        )
    } else if !result.author.is_empty() {
        format!("{} · {} {episodes}", result.author, result.episode_count)
    } else {
        format!("{} {episodes}", result.episode_count)
    }
}

/// Present the "add RSS feed" dialog parented to the panel's toplevel window;
/// a valid subscribe response routes to [`PodcastPanelInput::Subscribe`].
fn present_add_feed_dialog(root: &gtk::Box, sender: &ComponentSender<PodcastPanel>) {
    let Some(window) = root.root().and_downcast::<gtk::Window>() else {
        return;
    };

    let dialog = adw::Dialog::builder()
        .title(i18n("Add RSS Feed"))
        .content_width(440)
        .content_height(260)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);
    page.append_description(&i18n("Paste the podcast RSS feed URL below."));

    let group = BigPreferencesGroupSpec::new(i18n("Feed")).build();
    let feed_url_row = adw::EntryRow::builder()
        .title(i18n("RSS feed URL"))
        .show_apply_button(true)
        .build();
    feed_url_row.update_property(&[gtk::accessible::Property::Label(&i18n("RSS feed URL"))]);
    group.add(&feed_url_row);
    page.content().append(&group);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Subscribe")));
    footer.accept_button().set_sensitive(false);
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let accept_button_weak = footer.accept_button().downgrade();
    feed_url_row.connect_changed(move |row| {
        let Some(accept_button) = accept_button_weak.upgrade() else {
            return;
        };
        accept_button.set_sensitive(accepted_podcast_feed_url(&row.text()).is_some());
    });

    let accept_button_weak = footer.accept_button().downgrade();
    feed_url_row.connect_apply(move |row| {
        let Some(accept_button) = accept_button_weak.upgrade() else {
            return;
        };
        if accepted_podcast_feed_url(&row.text()).is_some() {
            accept_button.emit_clicked();
        }
    });

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let s = sender.clone();
    let dialog_weak = dialog.downgrade();
    let feed_url_row_for_accept = feed_url_row.clone();
    footer.accept_button().connect_clicked(move |_| {
        let Some(url) = accepted_podcast_feed_url(&feed_url_row_for_accept.text()) else {
            return;
        };
        s.input(PodcastPanelInput::Subscribe(url));
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    dialog.connect_map(move |_| {
        let feed_url_row = feed_url_row.downgrade();
        gtk::glib::idle_add_local_once(move || {
            if let Some(feed_url_row) = feed_url_row.upgrade() {
                feed_url_row.grab_focus();
            }
        });
    });
    dialog.present(Some(&window));
}

fn accepted_podcast_feed_url(url: &str) -> Option<String> {
    let url = url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    let (_, target) = url.split_once("://")?;
    if target.trim().is_empty() {
        return None;
    }
    Some(url.to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::{
        accepted_podcast_feed_url, is_subscribed_url, subscription_title_subtitle,
        title_fetch_candidates,
    };

    #[test]
    fn accepted_podcast_feed_url_trims_valid_http_inputs() {
        assert_eq!(
            accepted_podcast_feed_url("  https://example.test/feed.xml  "),
            Some("https://example.test/feed.xml".into())
        );
        assert_eq!(
            accepted_podcast_feed_url("http://example.test/feed.xml"),
            Some("http://example.test/feed.xml".into())
        );
    }

    #[test]
    fn accepted_podcast_feed_url_rejects_empty_or_non_http_inputs() {
        assert_eq!(accepted_podcast_feed_url(""), None);
        assert_eq!(accepted_podcast_feed_url("file:///tmp/feed.xml"), None);
        assert_eq!(accepted_podcast_feed_url("feed://example.test/rss"), None);
        assert_eq!(accepted_podcast_feed_url("https://   "), None);
    }

    #[test]
    fn title_fetch_candidates_skip_resolved_pending_and_failed_urls() {
        let subs = vec![
            "https://example.test/resolved.xml".into(),
            "https://example.test/pending.xml".into(),
            "https://example.test/failed.xml".into(),
            "https://example.test/new.xml".into(),
        ];
        let titles = HashMap::from([(
            "https://example.test/resolved.xml".into(),
            "Resolved".into(),
        )]);
        let pending = HashSet::from(["https://example.test/pending.xml".into()]);
        let failed = HashSet::from(["https://example.test/failed.xml".into()]);

        assert_eq!(
            title_fetch_candidates(&subs, &titles, &pending, &failed),
            vec!["https://example.test/new.xml".to_string()]
        );
    }

    #[test]
    fn subscription_title_subtitle_stops_loading_after_failed_fetch() {
        assert_eq!(subscription_title_subtitle(true, false), None);
        assert_eq!(
            subscription_title_subtitle(false, false),
            Some("Loading…".into())
        );
        assert_eq!(
            subscription_title_subtitle(false, true),
            Some("Title unavailable".into())
        );
    }

    #[test]
    fn is_subscribed_url_rejects_late_fetch_result_after_unsubscribe() {
        let subs = vec!["https://example.test/active.xml".into()];

        assert!(is_subscribed_url(&subs, "https://example.test/active.xml"));
        assert!(!is_subscribed_url(
            &subs,
            "https://example.test/removed.xml"
        ));
    }
}
