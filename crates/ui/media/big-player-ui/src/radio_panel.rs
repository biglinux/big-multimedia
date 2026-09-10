//! Relm4 radio tab — saved "My Stations" list, Radio Browser search/explore,
//! genre filter, add-station dialog.
//!
//! Rendering: ONE virtualized [`MediaListView`](crate::media_list) carries both
//! sections (My Stations + Explore) as a single recycling `ListView`, so only
//! the ~visible rows become widgets and only their favicons are fetched (in
//! `bind`, off-thread, cached). Replacing the data is a model swap, not a widget
//! rebuild; a track change updates only the realized rows' highlight. The old
//! build-all `ListBox` + per-row favicon fetch storm is gone.
//!
//! Push-model: API search/discovery (off-thread) + explore disk cache are
//! internal. Streaming + the now-playing panel are SHELL concerns: a row
//! activation emits `RadioPanelOutput::PlayStation`. Station persistence is the
//! shell's DB (emit Save/DeleteStation, RequestStations; receive
//! RefreshStations). Manual `impl Component` idiom; mirrors `podcast_panel.rs`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use relm4::{Component, ComponentParts, ComponentSender};

use crate::favicon_cache::{load_explore_cache, save_explore_cache};
use crate::media_list::MediaListView;
use big_media_data::db::models::RadioStation;
use big_media_data::i18n::i18n;
use big_media_data::radio::RadioExploreEntry;

mod methods;
mod widgets;

use methods::QueryKind;

/// Row tag: a saved "My Stations" entry (delete + record actions).
pub(super) const TAG_SAVED: u32 = 1;
/// Row tag: an Explore result (record + add actions).
pub(super) const TAG_EXPLORE: u32 = 2;

/// Messages accepted by [`RadioPanel`].
#[derive(Debug)]
pub enum RadioPanelInput {
    /// Shell pushed the saved-station list (initial load or after a CRUD persist).
    RefreshStations(Vec<RadioStation>),
    /// Render fetched/cached explore results (stores + persists explore cache).
    ShowExploreResults(Vec<RadioExploreEntry>),
    /// Async query finished, keyed by generation; stale gens are dropped.
    SearchFinished {
        /// Generation this result belongs to.
        generation: u32,
        /// Whether this was a popular/explore load (empty keeps the screen).
        popular: bool,
        /// Fetched results, or `None` on connection failure.
        results: Option<Vec<RadioExploreEntry>>,
    },
    /// Shell pushed the current playing URL + stream flag (drives row highlight).
    NowPlayingChanged {
        /// URL now playing.
        url: String,
        /// Whether the source is a live stream.
        is_stream: bool,
    },
    /// Debounced search-entry text changed (internal route).
    SearchQueryChanged(String),
    /// Run the actual search after the debounce elapsed (internal route).
    RunSearch(String),
    /// Genre dropdown selection (None = All Genres → popular).
    GenreSelected(Option<String>),
    /// Load popular/explore stations.
    LoadPopular,
    /// A station/explore row was activated → emit PlayStation.
    PlayClicked(String),
    /// Delete a saved station by its stream URL (resolves the DB id internally).
    DeleteStationByUrl(String),
    /// Add an explore/dialog station to My Stations (optimistic + emit).
    AddStation(RadioStation),
    /// Open the add-station dialog.
    AddDialogRequested,
    /// Record toggle: `true` = start, `false` = stop.
    RecordToggle(bool),
}

/// Intents emitted by the radio panel for the shell.
#[derive(Debug)]
pub enum RadioPanelOutput {
    /// Play a stream (shell loads mpv, shows now-playing panel, sets is_stream).
    PlayStation {
        /// Stream URL.
        url: String,
        /// Station display name.
        name: String,
        /// Favicon URL (may be empty).
        favicon: String,
    },
    /// Start recording the current stream.
    RecordStream,
    /// Stop recording the current stream.
    StopRecording,
    /// Persist a new saved station.
    SaveStation {
        /// Display name.
        name: String,
        /// Stream URL.
        url: String,
        /// Favicon URL.
        favicon: String,
        /// Group name (may be empty).
        group_name: String,
    },
    /// Delete a saved station by id.
    DeleteStation(i64),
    /// Ask the shell to (re)load saved stations from the DB.
    RequestStations,
    /// Transient notification toast.
    Notify(String),
}

/// Relm4 radio tab component.
pub struct RadioPanel {
    list: MediaListView,
    scroll: gtk::ScrolledWindow,
    empty_state: gtk::Box,
    spinner: adw::Spinner,
    skeleton: gtk::Box,
    stations: Vec<RadioStation>,
    explore_data: Vec<RadioExploreEntry>,
    /// Saved stream URLs, shared into the row-suffix builder so the Explore
    /// "add" button reflects the current saved set at bind time.
    saved_urls: Rc<RefCell<HashSet<String>>>,
    filter: String,
    search_gen: u32,
    search_debounce: Option<glib::SourceId>,
    current_url: String,
    is_stream: bool,
}

impl Component for RadioPanel {
    type Init = ();
    type Input = RadioPanelInput;
    type Output = RadioPanelOutput;
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
        let (toolbar, search_entry, genre_dropdown, add_button) = widgets::build_toolbar();
        root.append(&toolbar);
        root.append(
            &gtk::Separator::builder()
                .css_classes(["sidebar-separator"])
                .build(),
        );

        let empty_state = widgets::build_empty_state(&sender);
        root.append(&empty_state);
        let spinner = adw::Spinner::builder()
            .halign(gtk::Align::Center)
            .width_request(32)
            .height_request(32)
            .margin_top(16)
            .margin_bottom(16)
            .visible(false)
            .build();
        root.append(&spinner);
        let skeleton = widgets::build_skeleton();
        root.append(&skeleton);

        let saved_urls: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
        let list = MediaListView::new(
            {
                let sender = sender.clone();
                move |url| sender.input(RadioPanelInput::PlayClicked(url.to_owned()))
            },
            widgets::row_suffix_builder(&sender, &saved_urls),
        );
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list.widget)
            .visible(false)
            .build();
        root.append(&scroll);

        widgets::wire_toolbar(&search_entry, &genre_dropdown, &add_button, &sender);

        // Ask the shell for the saved stations + show cached explore immediately.
        let _ = sender.output(RadioPanelOutput::RequestStations);
        let s = sender.clone();
        glib::spawn_future_local(async move {
            let cached = gio::spawn_blocking(load_explore_cache)
                .await
                .unwrap_or_default();
            if !cached.is_empty() {
                s.input(RadioPanelInput::ShowExploreResults(cached));
            }
            s.input(RadioPanelInput::LoadPopular);
        });

        ComponentParts {
            model: Self {
                list,
                scroll,
                empty_state,
                spinner,
                skeleton,
                stations: Vec::new(),
                explore_data: Vec::new(),
                saved_urls,
                filter: String::new(),
                search_gen: 0,
                search_debounce: None,
                current_url: String::new(),
                is_stream: false,
            },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match message {
            RadioPanelInput::RefreshStations(stations) => {
                self.stations = stations;
                self.sync_saved_urls();
                self.rebuild_list();
            }
            RadioPanelInput::ShowExploreResults(results) => {
                self.explore_data.clone_from(&results);
                let for_cache = results;
                gio::spawn_blocking(move || save_explore_cache(&for_cache));
                self.rebuild_list();
            }
            RadioPanelInput::SearchFinished {
                generation,
                popular,
                results,
            } => {
                if generation != self.search_gen {
                    return;
                }
                self.hide_loading();
                match results {
                    Some(r) if !r.is_empty() => {
                        sender.input(RadioPanelInput::ShowExploreResults(r));
                    }
                    Some(_) if popular => {}
                    Some(_) => {
                        self.explore_data.clear();
                        self.rebuild_list();
                    }
                    None => self.show_connection_error(),
                }
            }
            RadioPanelInput::NowPlayingChanged { url, is_stream } => {
                self.current_url = url;
                self.is_stream = is_stream;
                // Highlight only — no rebuild, no full-tree walk.
                self.list.set_now_playing(if self.is_stream {
                    &self.current_url
                } else {
                    ""
                });
            }
            RadioPanelInput::SearchQueryChanged(query) => {
                if let Some(id) = self.search_debounce.take() {
                    id.remove();
                }
                self.filter = query.clone();
                if query.is_empty() {
                    self.search_gen = self.search_gen.wrapping_add(1);
                    self.explore_data.clear();
                    self.rebuild_list();
                    return;
                }
                self.rebuild_list();
                let s = sender.clone();
                let id = glib::timeout_add_local_once(
                    std::time::Duration::from_millis(500),
                    move || s.input(RadioPanelInput::RunSearch(query)),
                );
                self.search_debounce = Some(id);
            }
            RadioPanelInput::RunSearch(query) => {
                self.search_debounce.take();
                self.start_query(QueryKind::Name(query), &sender);
            }
            RadioPanelInput::GenreSelected(genre) => match genre {
                Some(tag) => self.start_query(QueryKind::Tag(tag), &sender),
                None => self.start_query(QueryKind::Popular, &sender),
            },
            RadioPanelInput::LoadPopular => self.start_query(QueryKind::Popular, &sender),
            RadioPanelInput::PlayClicked(url) => {
                let (name, favicon) = self.resolve_station_meta(&url);
                let _ = sender.output(RadioPanelOutput::PlayStation { url, name, favicon });
            }
            RadioPanelInput::DeleteStationByUrl(url) => {
                if let Some(station) = self.stations.iter().find(|s| s.url == url) {
                    let _ = sender.output(RadioPanelOutput::DeleteStation(station.id));
                }
                self.stations.retain(|station| station.url != url);
                self.sync_saved_urls();
                self.rebuild_list();
                let _ = sender.output(RadioPanelOutput::Notify(i18n("Radio station removed")));
            }
            RadioPanelInput::AddStation(station) => {
                let _ = sender.output(RadioPanelOutput::SaveStation {
                    name: station.name.clone(),
                    url: station.url.clone(),
                    favicon: station.favicon.clone(),
                    group_name: station.group_name.clone(),
                });
                self.stations.push(station);
                self.sync_saved_urls();
                self.rebuild_list();
            }
            RadioPanelInput::AddDialogRequested => widgets::present_add_dialog(root, &sender),
            RadioPanelInput::RecordToggle(start) => {
                let _ = sender.output(if start {
                    RadioPanelOutput::RecordStream
                } else {
                    RadioPanelOutput::StopRecording
                });
            }
        }
    }
}

impl RadioPanel {
    /// Refresh the shared saved-URL set the Explore "add" button reads.
    fn sync_saved_urls(&self) {
        let mut set = self.saved_urls.borrow_mut();
        set.clear();
        set.extend(self.stations.iter().map(|station| station.url.clone()));
    }

    /// Build the combined [`MediaRow`] list (My Stations + Explore sections) and
    /// swap it into the view; toggle the empty/loading state.
    fn rebuild_list(&self) {
        let rows = self.build_rows();
        let has_content = !rows.is_empty();
        self.list.set_items(rows);
        self.list.set_now_playing(if self.is_stream {
            &self.current_url
        } else {
            ""
        });

        let loading = self.spinner.is_visible();
        self.empty_state.set_visible(
            self.stations.is_empty()
                && self.filter.is_empty()
                && self.explore_data.is_empty()
                && !loading,
        );
        self.scroll.set_visible(has_content);
    }
}
