//! App-local Relm4 IPTV browser.
//!
//! Push-model component: a
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

mod cache;
pub mod channel_list;
pub mod favorites;
pub mod fetcher;
mod logos;
mod providers_dialog;
mod xtream;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

use adw::prelude::*;
use big_relm4_components::task::spawn_blocking_result;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::i18n::i18n;
use big_os_kit::keyring::SecretService;
use big_os_kit::url::ListOrigin;

use crate::media::iptv::{
    IptvChannel, IptvProvider, IptvProviderStore, MAX_PLAYLIST_ENTRIES, left_out_notice,
    parse_channel_list,
};

use channel_list::ChannelListView;
use fetcher::{
    ChannelUrls, FetchError, FetchedChannels, channel_urls, fetch_provider_channels,
    provider_identifier,
};

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
        channels: Result<FetchedChannels, FetchError>,
    },
    /// Plain-text secrets moved into the keyring: `(index, original,
    /// protected)` each, and whether every one moved.
    Protected {
        protected: Vec<(usize, IptvProvider, IptvProvider)>,
        complete: bool,
    },
    /// A channel could not play: the keyring kept its provider's password.
    KeyringUnavailable,
}

/// Intents emitted by the browser for the shell to act on.
#[derive(Debug)]
pub enum VideoIptvBrowserOutput {
    /// Play this channel (with a display title) in the main player; the
    /// provider's list named it.
    PlayChannel {
        urls: ChannelUrls,
        title: String,
        origin: ListOrigin,
    },
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
    /// Blocked while the dropdown is refilled, whose caller loads the provider.
    provider_selection: gtk::glib::SignalHandlerId,
    /// Status caption (cached/loading/error messaging).
    status: gtk::Label,
    /// Shown while a provider's password stays in the file for want of a
    /// keyring.
    keyring_status: gtk::Label,
    /// A move into the keyring is running; a second would store twice.
    protecting: bool,
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
        // A raw GtkDropDown derives its AT-SPI name from the selected item, so
        // with an empty provider list the `Label` property is overridden to ''
        // (screen-reader-invisible). Bind the visible "Provider" heading via the
        // labelled-by relation so the control keeps a stable accessible name
        // regardless of selection.
        provider_dropdown.update_property(&[gtk::accessible::Property::Label(&i18n("Provider"))]);
        big_gtk_kit::accessible::relate(
            &provider_dropdown,
            gtk::AccessibleRelation::LabelledBy,
            &[provider_label.upcast_ref()],
        );
        provider_row_box.append(&provider_dropdown);

        let manage_providers_button = gtk::Button::builder().label(i18n("Manage…")).build();
        provider_row_box.append(&manage_providers_button);

        let refresh_provider_button = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .build();
        refresh_provider_button.update_property(&[gtk::accessible::Property::Label(&i18n(
            "Reload from provider",
        ))]);
        provider_row_box.append(&refresh_provider_button);
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

        let keyring_status = gtk::Label::builder()
            .label(i18n(
                "Some provider passwords are still in a plain file because your keyring could not be reached. Unlock your keyring, then reload the channels.",
            ))
            .wrap(true)
            .selectable(true)
            .xalign(0.0)
            .margin_start(12)
            .margin_end(12)
            .css_classes(["caption", "warning"])
            .visible(false)
            .build();
        root.append(&keyring_status);

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
        list_view.status.set_margin_start(12);
        list_view.status.set_margin_end(12);
        root.append(&list_view.status);
        scrolled.set_child(Some(&list_view.root));
        root.append(&scrolled);

        let provider_selection = wire_widget_signals(
            &manage_providers_button,
            &refresh_provider_button,
            &search,
            &provider_dropdown,
            &sender,
        );

        let mut model = Self {
            store,
            list_view,
            provider_model,
            provider_dropdown,
            provider_selection,
            status,
            keyring_status,
            protecting: false,
            load_generation: 0,
        };
        model.protect_plaintext(&sender);
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
                self.protect_plaintext(&sender);
                let index = self.provider_dropdown.selected() as usize;
                self.load_provider(index, true, &sender);
            }
            VideoIptvBrowserInput::Search(query) => self.list_view.apply_filter(&query),
            VideoIptvBrowserInput::ManageProviders => self.open_providers_dialog(root, &sender),
            VideoIptvBrowserInput::ProvidersChanged => {
                self.protect_plaintext(&sender);
                self.on_providers_changed(&sender);
            }
            VideoIptvBrowserInput::PlayChannel { url, title } => {
                self.play_channel(url, title, &sender);
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
            VideoIptvBrowserInput::Protected {
                protected,
                complete,
            } => {
                self.protecting = false;
                let mut store = self.store.borrow_mut();
                if store.apply_protected(protected) {
                    store.save();
                }
                self.keyring_status.set_visible(!complete);
            }
            VideoIptvBrowserInput::KeyringUnavailable => {
                self.status
                    .set_label(&fetch_error_text(&FetchError::Keyring));
            }
        }
    }
}

impl VideoIptvBrowser {
    /// The providers as edited, for resolving saved channels off the main loop.
    #[must_use]
    pub fn providers(&self) -> Vec<IptvProvider> {
        self.store.borrow().providers.clone()
    }

    /// Move the secrets a provider still holds in plain text, typed into an
    /// older build or carried in an older file, into the keyring.
    fn protect_plaintext(&mut self, sender: &ComponentSender<Self>) {
        let unprotected = self.store.borrow().unprotected();
        if unprotected.is_empty() {
            self.keyring_status.set_visible(false);
        }
        if self.protecting || unprotected.is_empty() {
            return;
        }
        self.protecting = true;
        let sender = sender.clone();
        crate::settings::saving::acknowledged_task(
            true,
            move || {
                let cancelled = AtomicBool::new(false);
                let vault = SecretService {
                    cancelled: &cancelled,
                };
                let mut protected = Vec::new();
                for (index, original) in unprotected {
                    let mut provider = original.clone();
                    if provider.protect_secret(&vault).is_err() {
                        return Ok((protected, false));
                    }
                    // A redacted M3U URL names the cache differently; the old
                    // file would never be read or replaced again.
                    let cached_as = provider_identifier(&original);
                    if provider_identifier(&provider) != cached_as {
                        cache::remove(&cached_as);
                    }
                    protected.push((index, original, provider));
                }
                Ok((protected, true))
            },
            move |result| {
                let (protected, complete) = result.unwrap_or_default();
                sender.input(VideoIptvBrowserInput::Protected {
                    protected,
                    complete,
                });
            },
        );
    }

    /// Hand the channel at `url` of the selected provider to the shell, with
    /// the Xtream account's credentials read back from the keyring.
    fn play_channel(&self, url: String, title: String, sender: &ComponentSender<Self>) {
        let index = self.provider_dropdown.selected() as usize;
        let Some(provider) = self.store.borrow().providers.get(index).cloned() else {
            return;
        };
        let origin = provider.source.list_origin();
        let sender = sender.clone();
        spawn_blocking_result(
            move || {
                let cancelled = AtomicBool::new(false);
                let vault = SecretService {
                    cancelled: &cancelled,
                };
                channel_urls(&provider, &url, &vault)
            },
            move |urls: std::io::Result<ChannelUrls>| match urls {
                Ok(urls) => {
                    let _ = sender.output(VideoIptvBrowserOutput::PlayChannel {
                        urls,
                        title,
                        origin,
                    });
                }
                Err(_) => sender.input(VideoIptvBrowserInput::KeyringUnavailable),
            },
        );
    }

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
        self.provider_dropdown
            .block_signal(&self.provider_selection);
        self.provider_model.splice(0, n, &name_refs);
        if self.provider_model.n_items() > 0 {
            self.provider_dropdown.set_selected(0);
        }
        self.provider_dropdown
            .unblock_signal(&self.provider_selection);
    }

    /// Present the providers-management dialog over the browser root.
    fn open_providers_dialog(&self, root: &gtk::Box, sender: &ComponentSender<Self>) {
        providers_dialog::present_providers_dialog(root, self.store.clone(), sender.clone());
    }

    /// Reload the store after a CRUD edit and refresh the dropdown.
    fn on_providers_changed(&mut self, sender: &ComponentSender<Self>) {
        // The shared model already contains these edits; disk persistence may
        // still be pending. Reloading now would replace them with older bytes.
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
            spawn_cached_load(
                provider_id.clone(),
                provider.source.list_origin(),
                generation,
                sender,
            );
        }
        spawn_network_load(provider, provider_id, generation, sender);
    }

    /// Apply a finished network fetch (only for the current generation): replace
    /// the list on success, or surface a fallback message on failure.
    fn on_network_resolved(
        &mut self,
        generation: u64,
        fetched: Result<FetchedChannels, FetchError>,
    ) {
        if generation != self.load_generation {
            return;
        }
        match fetched {
            Ok(FetchedChannels {
                channels,
                favorite_renames,
            }) => {
                let mut status = Vec::new();
                if channels.truncated {
                    status.push(
                        i18n("Showing the first {} channels; the list has more.")
                            .replace("{}", &MAX_PLAYLIST_ENTRIES.to_string()),
                    );
                }
                status.extend(left_out_notice(false, channels.skipped));
                self.list_view.set_channels(channels.entries);
                self.list_view.rename_favorites(favorite_renames);
                self.status.set_label(&status.join(" "));
            }
            Err(error) => self.status.set_label(&fetch_error_text(&error)),
        }
    }
}

/// What went wrong, in words the person can act on. A list saved before
/// stays on screen.
pub(crate) fn fetch_error_text(error: &FetchError) -> String {
    match error {
        FetchError::Unreachable => i18n(
            "Could not reach the provider. Check the address and your connection; a saved list stays on screen.",
        ),
        FetchError::Status(401 | 403) => i18n(
            "The provider refused access. Check the user name and password, or the User-Agent the provider asks for.",
        ),
        FetchError::Status(status) if *status >= 500 => {
            i18n("The provider's server failed (error {}). Try again later.")
                .replace("{}", &status.to_string())
        }
        FetchError::Status(status) => {
            i18n("The provider answered with error {}.").replace("{}", &status.to_string())
        }
        FetchError::TooLarge => i18n("The channel list is larger than {} MB.").replace(
            "{}",
            &(crate::media::iptv::MAX_PLAYLIST_BYTES / (1024 * 1024)).to_string(),
        ),
        FetchError::Empty => i18n("The provider sent no channels."),
        FetchError::LoginRejected => i18n("The provider rejected the user name or password."),
        FetchError::AccountInactive(status) => {
            i18n("This account is not active (the provider says: {}).").replace("{}", status)
        }
        FetchError::FileUnreadable => i18n("Could not read the playlist file."),
        FetchError::Keyring => i18n(
            "Could not read this provider's password from your keyring. Unlock your keyring, then reload the channels.",
        ),
        FetchError::NoProvider => i18n(
            "This playlist has IPTV channels from a provider that is not set up. Add the provider under IPTV, then open the playlist again.",
        ),
    }
}

/// Read + parse the cached playlist off-thread, posting it back to the
/// component tagged with `generation`.
fn spawn_cached_load(
    provider_id: String,
    origin: ListOrigin,
    generation: u64,
    sender: &ComponentSender<VideoIptvBrowser>,
) {
    let sender = sender.clone();
    spawn_blocking_result(
        move || {
            cache::read(&provider_id).map(|content| parse_channel_list(&content, origin).entries)
        },
        move |channels| {
            if let Some(channels) = channels {
                sender.input(VideoIptvBrowserInput::CachedResolved {
                    generation,
                    channels,
                });
            }
        },
    );
}

/// Fetch + parse the provider's playlist over the network off-thread (caching
/// the raw body on success), posting the result back tagged with `generation`.
fn spawn_network_load(
    provider: IptvProvider,
    provider_id: String,
    generation: u64,
    sender: &ComponentSender<VideoIptvBrowser>,
) {
    let sender = sender.clone();
    spawn_blocking_result(
        move || fetch_provider_channels(&provider, &provider_id),
        move |channels| {
            sender.input(VideoIptvBrowserInput::NetworkResolved {
                generation,
                channels,
            });
        },
    );
}

/// Wire the static widget signals (manage/refresh/search/dropdown) to component
/// inputs, returning the dropdown's selection handler. Pulled out of `init` to
/// keep that function small.
fn wire_widget_signals(
    manage_providers_button: &gtk::Button,
    refresh_provider_button: &gtk::Button,
    search: &gtk::SearchEntry,
    provider_dropdown: &gtk::DropDown,
    sender: &ComponentSender<VideoIptvBrowser>,
) -> gtk::glib::SignalHandlerId {
    let manage_sender = sender.clone();
    manage_providers_button.connect_clicked(move |_| {
        manage_sender.input(VideoIptvBrowserInput::ManageProviders);
    });

    let refresh_sender = sender.clone();
    refresh_provider_button.connect_clicked(move |_| {
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
    })
}
