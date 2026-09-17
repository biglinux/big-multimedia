//! Grouped IPTV channel list — virtualized (Relm4 port).
//!
//! An IPTV M3U routinely carries thousands to tens of thousands of channels,
//! each with a remote `tvg-logo`. The old view built one `AdwActionRow` per
//! channel into a `ListBox` up front (and rebuilt the whole tree on every
//! keystroke of the filter), firing a logo fetch for every row whether or not
//! it was on screen — a widget-churn + fetch-storm that froze on large lists.
//!
//! This reuses the shared recycling [`MediaListView`]: a `GListModel` of
//! lightweight rows feeds a `ListView`, so only the ~visible rows ever become
//! widgets and only their logos are fetched (lazily, off-thread, cancelled on
//! recycle). Groups become inline section-header rows. Filtering is a cheap
//! model splice ([`MediaListView::set_items`]), never a widget rebuild.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use big_player_ui::media_list::{MediaListView, MediaRow, RowImage};
use big_relm4_components::feedback::tooltip;
use relm4::ComponentSender;

use crate::i18n::{i18n, ni18n};
use crate::media::iptv::IptvChannel;
use crate::video::ui::iptv::favorites::IptvFavorites;
use crate::video::ui::iptv::logos;
use crate::video::ui::iptv::{VideoIptvBrowser, VideoIptvBrowserInput};

/// Logo pixel size in the channel list.
const LOGO_PX: i32 = 40;

/// Grouped channel list widget plus the state needed to re-splice it on filter
/// or channel-set changes. Owned by the component; not a relm4 component itself.
pub struct ChannelListView {
    /// Root `ListView` mounted (in a `ScrolledWindow`) by the browser component.
    pub root: gtk::Widget,
    view: MediaListView,
    all_channels: Rc<RefCell<Vec<IptvChannel>>>,
    user_agent: Rc<RefCell<String>>,
    referer: Rc<RefCell<String>>,
    last_query: RefCell<String>,
}

impl ChannelListView {
    #[must_use]
    pub fn new(sender: ComponentSender<VideoIptvBrowser>) -> Self {
        let all_channels: Rc<RefCell<Vec<IptvChannel>>> = Rc::new(RefCell::new(Vec::new()));
        let favorites = Rc::new(RefCell::new(IptvFavorites::load()));

        // Activation: map the row id (channel URL) back to its title for play.
        let on_activate = {
            let all_channels = all_channels.clone();
            let sender = sender.clone();
            move |url: &str| {
                let title = all_channels
                    .borrow()
                    .iter()
                    .find(|c| c.url == url)
                    .map_or_else(|| url.to_string(), |c| c.name.clone());
                sender.input(VideoIptvBrowserInput::PlayChannel {
                    url: url.to_string(),
                    title,
                });
            }
        };

        // Per-row suffix: the favorite-toggle star (rebuilt on each visible bind).
        let build_suffix = {
            let favorites = favorites.clone();
            let sender = sender.clone();
            move |row: &MediaRow| -> Vec<gtk::Widget> {
                if row.header.is_some() || row.id.is_empty() {
                    return Vec::new();
                }
                let url = row.id.clone();
                let is_favorite = favorites.borrow().contains(&url);
                let favorite_button = tooltip::icon_toggle_button(
                    star_icon(is_favorite),
                    &i18n("Toggle favorite"),
                    is_favorite,
                    &["flat", "circular"],
                );
                favorite_button.set_valign(gtk::Align::Center);
                let favorites = favorites.clone();
                let sender = sender.clone();
                favorite_button.connect_toggled(move |button| {
                    let now_starred = favorites.borrow_mut().toggle(&url);
                    button.set_icon_name(star_icon(now_starred));
                    sender.input(VideoIptvBrowserInput::FavoriteToggled);
                });
                vec![favorite_button.upcast()]
            }
        };

        let view = MediaListView::new(on_activate, build_suffix);
        let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let status_store = favorites.clone();
        let retry_store = favorites.clone();
        wrapper.append(&crate::settings::saving::document_status(
            move || status_store.borrow().save_status(),
            move || retry_store.borrow().retry_save(),
        ));
        wrapper.append(&view.widget);
        Self {
            root: wrapper.upcast(),
            view,
            all_channels,
            user_agent: Rc::new(RefCell::new(String::new())),
            referer: Rc::new(RefCell::new(String::new())),
            last_query: RefCell::new(String::new()),
        }
    }

    /// Set the per-provider HTTP headers used for logo fetches.
    pub fn set_headers(&self, user_agent: &str, referer: &str) {
        self.user_agent.replace(user_agent.to_string());
        self.referer.replace(referer.to_string());
    }

    /// Replace the channel set and re-splice the (unfiltered) list.
    pub fn set_channels(&self, channels: Vec<IptvChannel>) {
        self.all_channels.replace(channels);
        self.last_query.replace(String::new());
        self.resplice("");
    }

    /// Re-render the list filtered by `query` (matches name or group). A model
    /// splice — only the visible rows rebind, no widget rebuild.
    pub fn apply_filter(&self, query: &str) {
        self.last_query.replace(query.to_string());
        self.resplice(query);
    }

    /// Re-splice so visible stars reflect cross-instance favorite toggles.
    pub fn reload_favorites(&self) {
        // Retain pending local edits. Cross-instance conflicts are explicit in
        // save status; a star toggle is not a request to discard this model.
        let query = self.last_query.borrow().clone();
        self.resplice(&query);
    }

    /// Build the flat row model (group headers + channel rows) for `query` and
    /// splice it into the view.
    fn resplice(&self, query: &str) {
        let query_lower = query.trim().to_lowercase();
        let channels = self.all_channels.borrow();
        let filtered: Vec<&IptvChannel> = channels
            .iter()
            .filter(|c| {
                query_lower.is_empty()
                    || c.name.to_lowercase().contains(&query_lower)
                    || c.group.to_lowercase().contains(&query_lower)
            })
            .collect();

        if filtered.is_empty() {
            self.view
                .set_items(vec![MediaRow::header(i18n("No channels"))]);
            return;
        }

        let user_agent = self.user_agent.borrow().clone();
        let referer = self.referer.borrow().clone();
        let mut rows = Vec::new();
        for (group_name, list) in group_channels(&filtered) {
            let subtitle = ni18n("{} channel", "{} channels", list.len() as u64)
                .replace("{}", &list.len().to_string());
            rows.push(MediaRow::header(format!("{group_name}  ·  {subtitle}")));
            for ch in list {
                rows.push(channel_row(ch, &user_agent, &referer));
            }
        }
        self.view.set_items(rows);
    }
}

/// One channel row: title/group, a remote logo (initials fallback) fetched
/// off-thread for visible rows only, keyed on the channel URL for activation.
fn channel_row(channel: &IptvChannel, user_agent: &str, referer: &str) -> MediaRow {
    let image = if channel.logo.is_empty() {
        RowImage::Icon {
            name: "video-x-generic-symbolic".to_owned(),
            size: LOGO_PX,
        }
    } else {
        let ua = user_agent.to_owned();
        let referer = referer.to_owned();
        RowImage::Remote {
            url: channel.logo.clone(),
            initials: channel.name.clone(),
            size: LOGO_PX,
            fetch: Arc::new(move |url: &str| logos::fetch_bytes(url, &ua, &referer)),
        }
    };
    MediaRow {
        id: channel.url.clone(),
        title: channel.name.clone(),
        subtitle: (!channel.group.is_empty()).then(|| channel.group.clone()),
        image,
        header: None,
        favicon: String::new(),
        group: channel.group.clone(),
        tag: 0,
    }
}

/// Group filtered channels by `group-title`, preserving first-seen order;
/// empty groups fold into a single translated "Ungrouped" bucket.
fn group_channels<'a>(filtered: &[&'a IptvChannel]) -> Vec<(String, Vec<&'a IptvChannel>)> {
    let mut grouped: Vec<(String, Vec<&IptvChannel>)> = Vec::new();
    for ch in filtered {
        let key = if ch.group.is_empty() {
            i18n("Ungrouped")
        } else {
            ch.group.clone()
        };
        match grouped.iter_mut().find(|(k, _)| k == &key) {
            Some((_, list)) => list.push(ch),
            None => grouped.push((key, vec![ch])),
        }
    }
    grouped
}

fn star_icon(is_favorite: bool) -> &'static str {
    if is_favorite {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    }
}
