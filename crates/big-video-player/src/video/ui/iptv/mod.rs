//! App-local Relm4 IPTV browser (video port, V4).
//!
//! Reproduces the legacy `iptv_view` browsing UX in the push-model idiom: a
//! provider dropdown (backed by the persistent `IptvProviderStore`), a
//! "Manage…" CRUD dialog, a live search filter, a refresh button, and a grouped
//! channel list with cached logos and per-channel favorites. Playing a channel
//! emits `VideoIptvBrowserOutput::PlayChannel`; the shell routes the URL to
//! mpv. The browser owns no playback state.
//!
//! Async discipline (mirrors `big_audio_player`'s async→self pattern): every
//! network / disk fetch runs on a worker thread and posts its result back to
//! this component's own `VideoIptvBrowserInput`; the GTK main loop never
//! blocks. A monotonically increasing `load_generation` guards against stale
//! results from a superseded provider selection landing in the list.
//!
//! Video-local: IPTV is genuinely player-specific, so the composition lives in
//! the app, reusing shared rows (`big-relm4-components`) and the runtime IPTV
//! data model (`big-media-runtime::media::iptv`) rather than rewriting them.
//! Mirrors / supersedes the legacy `iptv_view` dialog; the legacy stays until
//! the V6 cutover.

mod cache;
pub mod channel_list;
pub mod favorites;
pub mod fetcher;
mod logos;
mod providers_dialog;
mod xtream;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::i18n::i18n;
use crate::media::iptv::{parse_m3u_content, IptvChannel, IptvProvider, IptvProviderStore};

use channel_list::ChannelListView;
use fetcher::{fetch_provider_channels, provider_identifier};

/// Messages into the IPTV browser.
#[derive(Debug)]
pub enum VideoIptvBrowserInput {
    /// The provider dropdown changed to `index` (load it, cache-first).
    ProviderSelected(usize),
    /// The refresh button was clicked (re-fetch the current provider).
    Refresh,
    /// The live search query changed.
    Search(String),
    /// Open the providers-management dialog.
    ManageProviders,
    /// The providers dialog closed → reload the store + dropdown.
    ProvidersChanged,
    /// A channel row was activated → forward the play intent to the shell.
    PlayChannel { url: String, title: String },
    /// A favorite star toggled → re-sync the list's favorites view.
    FavoriteToggled,
    /// Cached channels resolved off-thread for load generation `gen`.
    CachedResolved {
        generation: u64,
        channels: Vec<IptvChannel>,
    },
    /// Network channels resolved off-thread for load generation `gen`.
    NetworkResolved {
        generation: u64,
        channels: Option<Vec<IptvChannel>>,
    },
}

/// Intents emitted by the browser for the shell to act on.
#[derive(Debug)]
pub enum VideoIptvBrowserOutput {
    /// Play this stream URL (with a display title) in the main player.
    PlayChannel { url: String, title: String },
}

/// Relm4 IPTV browser component.
pub struct VideoIptvBrowser {
    /// Persistent provider store (loaded at init, reloaded on CRUD close).
    store: Rc<RefCell<IptvProviderStore>>,
    /// Grouped channel list view (owned widget builder, not a relm4 child).
    list_view: ChannelListView,
    /// Provider dropdown model + widget (kept for repopulation on CRUD close).
    provider_model: gtk::StringList,
    provider_dropdown: gtk::DropDown,
    /// Status caption (cached/loading/error messaging).
    status: gtk::Label,
    /// Monotonic load token; off-thread results for an older value are dropped.
    load_generation: u64,
}

impl Component for VideoIptvBrowser {
    type Init = ();
    type Input = VideoIptvBrowserInput;
    type Output = VideoIptvBrowserOutput;
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
        let store = Rc::new(RefCell::new(IptvProviderStore::load()));

        let provider_row_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .build();
        let provider_label = gtk::Label::builder()
            .label(i18n("Provider"))
            .css_classes(["heading"])
            .xalign(0.0)
            .build();
        provider_row_box.append(&provider_label);

        let provider_model = gtk::StringList::new(&[]);
        let provider_dropdown = gtk::DropDown::builder()
            .model(&provider_model)
            .hexpand(true)
            .build();
        provider_dropdown.update_property(&[gtk::accessible::Property::Label(&i18n("Provider"))]);
        provider_row_box.append(&provider_dropdown);

        let manage_btn = gtk::Button::builder().label(i18n("Manage…")).build();
        provider_row_box.append(&manage_btn);

        let refresh_btn = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .build();
        refresh_btn.update_property(&[gtk::accessible::Property::Label(&i18n(
            "Reload from provider",
        ))]);
        provider_row_box.append(&refresh_btn);
        root.append(&provider_row_box);

        let search = gtk::SearchEntry::builder()
            .placeholder_text(i18n("Search channels"))
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .build();
        search.update_property(&[gtk::accessible::Property::Label(&i18n("Search channels"))]);
        root.append(&search);

        let status = gtk::Label::builder()
            .xalign(0.0)
            .margin_start(12)
            .margin_end(12)
            .css_classes(["dim-label", "caption"])
            .build();
        root.append(&status);

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .min_content_height(420)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(12)
            .build();
        let list_view = ChannelListView::new(sender.clone());
        scrolled.set_child(Some(&list_view.root));
        root.append(&scrolled);

        wire_widget_signals(
            &manage_btn,
            &refresh_btn,
            &search,
            &provider_dropdown,
            &sender,
        );

        let model = Self {
            store,
            list_view,
            provider_model,
            provider_dropdown,
            status,
            load_generation: 0,
        };
        model.repopulate_provider_dropdown();
        if model.provider_model.n_items() > 0 {
            sender.input(VideoIptvBrowserInput::ProviderSelected(0));
        } else {
            model.status.set_label(&i18n(
                "No providers configured. Click \u{201C}Manage\u{2026}\u{201D} to add one.",
            ));
        }

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match message {
            VideoIptvBrowserInput::ProviderSelected(index) => {
                self.load_provider(index, false, &sender);
            }
            VideoIptvBrowserInput::Refresh => {
                let index = self.provider_dropdown.selected() as usize;
                self.load_provider(index, true, &sender);
            }
            VideoIptvBrowserInput::Search(query) => self.list_view.apply_filter(&query),
            VideoIptvBrowserInput::ManageProviders => self.open_providers_dialog(root, &sender),
            VideoIptvBrowserInput::ProvidersChanged => self.on_providers_changed(&sender),
            VideoIptvBrowserInput::PlayChannel { url, title } => {
                let _ = sender.output(VideoIptvBrowserOutput::PlayChannel { url, title });
            }
            VideoIptvBrowserInput::FavoriteToggled => self.list_view.reload_favorites(),
            VideoIptvBrowserInput::CachedResolved {
                generation,
                channels,
            } => {
                if generation == self.load_generation {
                    self.list_view.set_channels(channels);
                    self.status
                        .set_label(&i18n("Showing cached channels — updating…"));
                }
            }
            VideoIptvBrowserInput::NetworkResolved {
                generation,
                channels,
            } => self.on_network_resolved(generation, channels),
        }
    }
}

impl VideoIptvBrowser {
    /// Refresh the dropdown model from the current store, selecting the first
    /// provider if any exist.
    fn repopulate_provider_dropdown(&self) {
        // StringList has no retain; splice replaces the range [0, n_items).
        let n = self.provider_model.n_items();
        let names: Vec<String> = self
            .store
            .borrow()
            .providers
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        self.provider_model.splice(0, n, &name_refs);
        if self.provider_model.n_items() > 0 {
            self.provider_dropdown.set_selected(0);
        }
    }

    /// Present the providers-management dialog over the browser root.
    fn open_providers_dialog(&self, root: &gtk::Box, sender: &ComponentSender<Self>) {
        providers_dialog::present_providers_dialog(root, self.store.clone(), sender.clone());
    }

    /// Reload the store after a CRUD edit and refresh the dropdown.
    fn on_providers_changed(&mut self, sender: &ComponentSender<Self>) {
        *self.store.borrow_mut() = IptvProviderStore::load();
        self.repopulate_provider_dropdown();
        if self.provider_model.n_items() > 0 {
            sender.input(VideoIptvBrowserInput::ProviderSelected(0));
        } else {
            self.list_view.set_channels(Vec::new());
            self.status.set_label(&i18n(
                "No providers configured. Click \u{201C}Manage\u{2026}\u{201D} to add one.",
            ));
        }
    }

    /// Load a provider's channels: show the cached copy immediately (unless a
    /// forced reload), then fetch fresh in the background. Both fetches run
    /// off-thread and post back to this component, tagged with the current load
    /// generation so a superseded selection's results are discarded.
    fn load_provider(&mut self, index: usize, force_network: bool, sender: &ComponentSender<Self>) {
        let Some(provider) = self.store.borrow().providers.get(index).cloned() else {
            self.list_view.set_channels(Vec::new());
            return;
        };
        self.list_view
            .set_headers(&provider.user_agent, &provider.referer);

        self.load_generation += 1;
        let generation = self.load_generation;
        let provider_id = provider_identifier(&provider);

        if force_network {
            self.status.set_label(&i18n("Reloading channels…"));
        } else {
            self.status.set_label(&i18n("Loading channels…"));
            spawn_cached_load(provider_id.clone(), generation, sender);
        }
        spawn_network_load(provider, provider_id, generation, sender);
    }

    /// Apply a finished network fetch (only for the current generation): replace
    /// the list on success, or surface a fallback message on failure.
    fn on_network_resolved(&mut self, generation: u64, channels: Option<Vec<IptvChannel>>) {
        if generation != self.load_generation {
            return;
        }
        match channels {
            Some(channels) => {
                self.list_view.set_channels(channels);
                self.status.set_label("");
            }
            None => self.status.set_label(&i18n(
                "Could not reach provider — showing cached copy if available.",
            )),
        }
    }
}

/// Read + parse the cached playlist off-thread, posting it back to the
/// component tagged with `generation`.
fn spawn_cached_load(
    provider_id: String,
    generation: u64,
    sender: &ComponentSender<VideoIptvBrowser>,
) {
    let (tx, rx) = async_channel::bounded::<Option<Vec<IptvChannel>>>(1);
    std::thread::spawn(move || {
        let result = cache::read(&provider_id).map(|content| parse_m3u_content(&content));
        let _ = tx.send_blocking(result);
    });
    let sender = sender.clone();
    glib::spawn_future_local(async move {
        if let Ok(Some(channels)) = rx.recv().await {
            sender.input(VideoIptvBrowserInput::CachedResolved {
                generation,
                channels,
            });
        }
    });
}

/// Fetch + parse the provider's playlist over the network off-thread (caching
/// the raw body on success), posting the result back tagged with `generation`.
fn spawn_network_load(
    provider: IptvProvider,
    provider_id: String,
    generation: u64,
    sender: &ComponentSender<VideoIptvBrowser>,
) {
    let (tx, rx) = async_channel::bounded::<Option<Vec<IptvChannel>>>(1);
    std::thread::spawn(move || {
        let result = fetch_provider_channels(&provider, &provider_id);
        let _ = tx.send_blocking(result);
    });
    let sender = sender.clone();
    glib::spawn_future_local(async move {
        let channels = rx.recv().await.ok().flatten();
        sender.input(VideoIptvBrowserInput::NetworkResolved {
            generation,
            channels,
        });
    });
}

/// Wire the static widget signals (manage/refresh/search/dropdown) to component
/// inputs. Pulled out of `init` to keep that function small.
fn wire_widget_signals(
    manage_btn: &gtk::Button,
    refresh_btn: &gtk::Button,
    search: &gtk::SearchEntry,
    provider_dropdown: &gtk::DropDown,
    sender: &ComponentSender<VideoIptvBrowser>,
) {
    let manage_sender = sender.clone();
    manage_btn.connect_clicked(move |_| {
        manage_sender.input(VideoIptvBrowserInput::ManageProviders);
    });

    let refresh_sender = sender.clone();
    refresh_btn.connect_clicked(move |_| {
        refresh_sender.input(VideoIptvBrowserInput::Refresh);
    });

    let search_sender = sender.clone();
    search.connect_search_changed(move |entry| {
        search_sender.input(VideoIptvBrowserInput::Search(entry.text().to_string()));
    });

    let dropdown_sender = sender.clone();
    provider_dropdown.connect_selected_notify(move |dropdown| {
        dropdown_sender.input(VideoIptvBrowserInput::ProviderSelected(
            dropdown.selected() as usize
        ));
    });
}
