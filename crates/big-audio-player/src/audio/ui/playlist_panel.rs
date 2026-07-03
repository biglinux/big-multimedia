//! App-local Relm4 playlist/queue panel (push-model).
//!
//! The queue is shell-owned: this component renders `SetQueue`/`SetPlaying`
//! snapshots and emits intents (`JumpTo`/`RemoveTracks`/`RateTrack`). It NEVER
//! rebuilds the `ColumnView` — data changes splice the backing `ListStore` in
//! place (rebuilding leaks the recycled row pool; the documented RSS regression).
//! The now-playing indicator matches by OBJECT IDENTITY so duplicate tracks in
//! the queue light independently.
//!
//! Implements: render core (splice), full column set, external-path drop,
//! single-source row reorder, Delete/Backspace multi-remove, column-header sort
//! commit, right-click context menu, toolbar (save-as/dedupe/clear), and the
//! empty-state placeholder (drop hint vs. first-run welcome). Saved-playlist
//! load, smart playlists, and import/export belong to the LIBRARY panel (they
//! populate the library detail view, not the queue). The column-config menu +
//! column-width persistence land in follow-up commits; queue mutation + undo
//! stay 100% shell-side.

use adw::prelude::*;
use gtk::{gio, glib};
use relm4::{Component, ComponentParts, ComponentSender};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use big_relm4_components::layout::column_chrome::wire_configure_columns_menu;
use big_relm4_components::layout::column_persist::{
    apply_column_widths, wire_column_width_persist,
};
use big_relm4_components::layout::toolbar_pane::{BigToolbarPane, BigToolbarPaneSpec};

use crate::audio::ui::{playlist_chrome, playlist_columns, playlist_interactions};
use crate::db::models::Track;
use crate::i18n::i18n;
use crate::shared::ui_helpers::format_duration_hm;

/// Live now-playing indicator widgets for one bound row, kept so `SetPlaying`
/// can re-light rows by object identity without rebinding the factory.
pub(super) type RowIndicator = (glib::Object, gtk::Image, gtk::Label);

fn playlist_track_objects(tracks: &[Track]) -> Vec<glib::Object> {
    tracks
        .iter()
        .map(|track| glib::BoxedAnyObject::new(track.clone()).upcast())
        .collect()
}

fn is_current_indicator_track(
    current_track_identity: Option<&glib::Object>,
    track_identity: &glib::Object,
) -> bool {
    is_current_indicator_identity_match(
        current_track_identity.is_some(),
        current_track_identity == Some(track_identity),
    )
}

fn is_current_indicator_identity_match(
    has_current_track_identity: bool,
    is_same_track_identity: bool,
) -> bool {
    has_current_track_identity && is_same_track_identity
}

fn scroll_target_position(track_count: usize, current_index: usize) -> Option<u32> {
    if track_count == 0 {
        None
    } else {
        Some((current_index + 3).min(track_count.saturating_sub(1)) as u32)
    }
}

fn insert_tracks_position(requested_position: usize, existing_count: u32) -> u32 {
    (requested_position as u32).min(existing_count)
}

fn should_refresh_empty_placeholder(track_count: u32) -> bool {
    track_count == 0
}

fn should_remove_queue_row(index: usize, track_count: u32) -> bool {
    (index as u32) < track_count
}

fn playlist_header_label(track_count: u32, total_secs: f64, playlist_name: &str) -> String {
    if track_count == 0 {
        if playlist_name.is_empty() {
            i18n("Playlist")
        } else {
            playlist_name.to_string()
        }
    } else {
        format!(
            "{} {} · {}",
            track_count,
            if track_count == 1 {
                i18n("track")
            } else {
                i18n("tracks")
            },
            format_duration_hm(total_secs),
        )
    }
}

/// Messages the shell pushes into the playlist panel.
#[derive(Debug)]
pub enum PlaylistPanelInput {
    /// Full queue snapshot (initial load, or after a mutation the shell applied).
    SetQueue {
        /// Ordered queue tracks.
        tracks: Vec<Track>,
        /// Index of the currently-playing track, if any.
        current_idx: Option<usize>,
        /// Active saved-playlist name (empty = ad-hoc queue).
        playlist_name: String,
    },
    /// Incremental append (fast path; avoids a full `SetQueue`).
    AppendTracks(Vec<Track>),
    /// Incremental insert at a queue position.
    InsertTracksAt {
        /// Insert position.
        pos: usize,
        /// Tracks to insert.
        tracks: Vec<Track>,
    },
    /// Incremental single-row removal.
    RemoveRow(usize),
    /// Current-track changed (track-identity re-light; no rebuild).
    SetPlaying {
        /// New current index, if any.
        idx: Option<usize>,
    },
    /// Scroll the current row into view (+3 look-ahead), deferred to idle.
    ScrollToCurrent {
        /// Current index to scroll to.
        idx: usize,
    },
    /// Active saved-playlist name changed.
    SetPlaylistName(String),
    /// Whether the library has any tracks — picks the empty-queue placeholder
    /// (drop hint vs. first-run welcome).
    SetLibraryHasItems(bool),
    /// Restore saved column widths (`title=width,…`), pushed by the shell at
    /// construction. Suppresses the persist echo while applying.
    ApplyColumnWidths(String),
}

/// Intents the playlist panel emits for the shell to act on.
#[derive(Debug)]
pub enum PlaylistPanelOutput {
    /// Play the queue track at `index` (row activated).
    JumpTo {
        /// Queue index to jump to.
        index: usize,
    },
    /// Remove queue tracks at these indices (descending order).
    RemoveTracks {
        /// Indices to remove, highest-first.
        indices: Vec<usize>,
    },
    /// Move a queue track (single-source row reorder via drag).
    ReorderTrack {
        /// Source queue index.
        from: usize,
        /// Destination queue index.
        to: usize,
    },
    /// User sorted by a column header — commit the visible order as the new
    /// queue (the shell replaces the queue with `tracks` + records undo).
    CommitSortedOrder {
        /// Tracks in the new (sorted) visible order.
        tracks: Vec<Track>,
    },
    /// Persist a new rating for a track (0–5 stars).
    RateTrack {
        /// Track DB id.
        id: i64,
        /// New rating 1–5.
        rating: i32,
    },
    /// Show the track-info dialog (shell owns the dialog + metadata).
    ShowTrackInfo(Track),
    /// Show the "add to playlist" dialog for a track.
    ShowAddToPlaylistDialog(Track),
    /// Reveal a file in the desktop file manager.
    RevealInFileManager(String),
    /// Download podcast episodes — `(url, title)` pairs (HTTP entries only).
    DownloadEpisodes(Vec<(String, String)>),
    /// Delete tracks from disk (trash or permanent) — shell does the fs delete,
    /// queue removal, DB delete, undo, save, and library rebuild.
    DeleteTracksFromStorage {
        /// `(queue_index, track)` pairs to delete.
        items: Vec<(usize, Track)>,
        /// `true` = permanent (skip trash).
        permanent: bool,
    },
    /// Launch the external converter on these file paths.
    ConvertTracks(Vec<String>),
    /// Clear the queue (toolbar). `with_undo` records an undo snapshot.
    ClearQueue {
        /// Whether the user already confirmed (toolbar clear is immediate+undoable).
        confirmed: bool,
        /// Push an undo snapshot before clearing.
        with_undo: bool,
    },
    /// Drop duplicate queue entries (toolbar).
    RemoveDuplicates,
    /// Save the current queue as a named playlist (shell reads its queue).
    SaveQueueAsPlaylist {
        /// User-entered playlist name.
        name: String,
    },
    /// External file paths dropped onto the queue at `insert_pos` (append when
    /// it equals the queue length). The shell resolves paths → tracks (DB),
    /// inserts, autoplays if idle, records undo, and persists.
    DropPaths {
        /// Queue index to insert at.
        insert_pos: usize,
        /// Dropped filesystem paths.
        paths: Vec<String>,
    },
    /// First-run "Open Files" picker chose audio files — shell imports + plays.
    OpenFiles(Vec<String>),
    /// Persist the queue column widths (`title=width,…`) — the shell writes the
    /// settings key. Debounced to one emission per resize gesture.
    PersistColumnWidths(String),
}

/// Relm4 playlist/queue panel component.
pub struct PlaylistPanel {
    store: gio::ListStore,
    column_view: gtk::ColumnView,
    /// Scroll area hosting the `ColumnView`; swapped for an empty-state widget
    /// when the queue is empty (never rebuilt — the view is reattached).
    scroll: gtk::ScrolledWindow,
    /// Toolbar header label: playlist name when empty, "N tracks · duration"
    /// when the queue has items.
    header_label: gtk::Label,
    /// Bound now-playing indicators, pruned on unbind.
    indicators: Rc<RefCell<Vec<RowIndicator>>>,
    /// The currently-playing store object (identity target for the indicator).
    current_track_identity: Rc<RefCell<Option<glib::Object>>>,
    /// Active saved-playlist name (shown in the toolbar header when the queue
    /// is empty).
    playlist_name: String,
    /// Library-has-tracks flag — picks the empty-queue placeholder variant.
    library_has_items: bool,
    /// Set while an inbound `ApplyColumnWidths` runs, so the resize-persist
    /// watcher doesn't echo restored widths straight back to the shell.
    applying_widths: Rc<Cell<bool>>,
}

impl PlaylistPanel {
    /// Wrap queue tracks as the `BoxedAnyObject`s the store/factories expect.
    fn to_objects(tracks: &[Track]) -> Vec<glib::Object> {
        playlist_track_objects(tracks)
    }

    /// Re-light the now-playing indicator on every bound row by object identity.
    fn relight(&self) {
        let current_track_identity = self.current_track_identity.borrow();
        for (track_identity, icon, label) in self.indicators.borrow().iter() {
            let is_current =
                is_current_indicator_track(current_track_identity.as_ref(), track_identity);
            icon.set_visible(is_current);
            if is_current {
                label.add_css_class("now-playing-accent");
            } else {
                label.remove_css_class("now-playing-accent");
            }
        }
    }

    /// Select + scroll the current row into view (+3 look-ahead), deferred until
    /// the view has realized.
    fn scroll_to(&self, idx: usize) {
        let count = self.store.n_items() as usize;
        let Some(target) = scroll_target_position(count, idx) else {
            return;
        };
        if let Some(selection) = self
            .column_view
            .model()
            .and_downcast::<gtk::MultiSelection>()
        {
            selection.select_item(idx as u32, true);
        }
        let cv = self.column_view.clone();
        glib::idle_add_local_once(move || {
            cv.scroll_to(target, None, gtk::ListScrollFlags::FOCUS, None);
        });
    }

    /// Refresh the toolbar header: the playlist name (or "Playlist") when the
    /// queue is empty, otherwise "N tracks · total duration".
    fn update_header(&self) {
        let count = self.store.n_items();
        let total_secs: f64 = (0..count)
            .filter_map(|i| self.store.item(i).and_downcast::<glib::BoxedAnyObject>())
            .map(|boxed_track| boxed_track.borrow::<Track>().duration_secs)
            .sum();
        self.header_label.set_label(&playlist_header_label(
            count,
            total_secs,
            &self.playlist_name,
        ));
    }

    /// Show the empty-state placeholder when the queue is empty, otherwise
    /// (re)attach the `ColumnView`. The view is reattached, never rebuilt.
    fn refresh_empty_state(&self, sender: &ComponentSender<Self>) {
        if self.store.n_items() == 0 {
            let empty = playlist_chrome::build_empty_state(self.library_has_items, sender);
            self.scroll.set_child(Some(&empty));
        } else {
            let cv: gtk::Widget = self.column_view.clone().upcast();
            if self.scroll.child().as_ref() != Some(&cv) {
                self.scroll.set_child(Some(&self.column_view));
            }
        }
    }
}

impl Component for PlaylistPanel {
    type Init = ();
    type Input = PlaylistPanelInput;
    type Output = PlaylistPanelOutput;
    type CommandOutput = ();
    type Root = adw::ToolbarView;
    type Widgets = ();

    fn init_root() -> Self::Root {
        BigToolbarPane::new(BigToolbarPaneSpec::new()).into_root()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        // SortListModel keeps clickable column-header sorting available.
        let sort_model = gtk::SortListModel::new(Some(store.clone()), None::<gtk::Sorter>);
        let selection = gtk::MultiSelection::new(Some(sort_model.clone()));

        let column_view = gtk::ColumnView::builder()
            .model(&selection)
            // Column-header drag-reorder; row reorder is custom (later commit).
            .reorderable(true)
            .show_column_separators(false)
            .show_row_separators(false)
            .vexpand(true)
            .build();
        column_view.add_css_class("playlist-columnview");

        let indicators: Rc<RefCell<Vec<RowIndicator>>> = Rc::new(RefCell::new(Vec::new()));
        let current_track_identity: Rc<RefCell<Option<glib::Object>>> = Rc::new(RefCell::new(None));

        playlist_columns::build_columns(
            &column_view,
            &indicators,
            &current_track_identity,
            &sender,
        );
        wire_configure_columns_menu(&column_view, &i18n("Configure Columns…"), &i18n("Columns"));
        let applying_widths = Rc::new(Cell::new(false));
        {
            let out = sender.clone();
            wire_column_width_persist(&column_view, &applying_widths, move |spec| {
                let _ = out.output(PlaylistPanelOutput::PersistColumnWidths(spec));
            });
        }

        // Row activation → JumpTo (shell loads + plays).
        {
            let out = sender.clone();
            column_view.connect_activate(move |_, position| {
                let _ = out.output(PlaylistPanelOutput::JumpTo {
                    index: position as usize,
                });
            });
        }

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&column_view)
            .build();

        // Drop-position indicator line, overlaid on the scroll area.
        let drop_indicator = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .height_request(2)
            .valign(gtk::Align::Start)
            .visible(false)
            .build();
        drop_indicator.add_css_class("drop-indicator");

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&scroll));
        overlay.add_overlay(&drop_indicator);

        let (toolbar, header_label) = playlist_chrome::build_toolbar(&sender);
        root.add_top_bar(&toolbar);
        root.set_content(Some(&overlay));

        playlist_interactions::wire_path_drop(
            &scroll,
            &column_view,
            &store,
            &drop_indicator,
            &sender,
        );

        // Single-source row reorder: DragSource carries the source index, the
        // DropTarget resolves the destination and emits ReorderTrack.
        column_view.add_controller(playlist_interactions::build_reorder_drag_source(&selection));
        playlist_interactions::wire_reorder_drop(
            &scroll,
            &column_view,
            &store,
            &drop_indicator,
            &sender,
        );
        playlist_interactions::wire_delete_key(
            &column_view,
            &selection,
            &sort_model,
            &store,
            &sender,
        );
        playlist_interactions::wire_sort_commit(&column_view, &sort_model, &sender);
        playlist_interactions::wire_right_click_menu(
            &column_view,
            &selection,
            &sort_model,
            &store,
            &scroll,
            &sender,
        );

        let model = Self {
            store,
            column_view,
            scroll,
            header_label,
            indicators,
            current_track_identity,
            playlist_name: String::new(),
            library_has_items: false,
            applying_widths,
        };
        // Empty queue at boot → show the placeholder until the shell pushes a
        // SetQueue (which reattaches the ColumnView).
        model.refresh_empty_state(&sender);
        model.update_header();

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            PlaylistPanelInput::SetQueue {
                tracks,
                current_idx,
                playlist_name,
            } => {
                self.playlist_name = playlist_name;
                let objects = Self::to_objects(&tracks);
                // Splice in place — never rebuild the ColumnView (RSS regression).
                self.store.splice(0, self.store.n_items(), &objects);
                *self.current_track_identity.borrow_mut() =
                    current_idx.and_then(|i| self.store.item(i as u32));
                self.refresh_empty_state(&sender);
                self.update_header();
                self.relight();
                if let Some(idx) = current_idx {
                    self.scroll_to(idx);
                }
            }
            PlaylistPanelInput::AppendTracks(tracks) => {
                let objects = Self::to_objects(&tracks);
                let n = self.store.n_items();
                self.store.splice(n, 0, &objects);
                self.refresh_empty_state(&sender);
                self.update_header();
            }
            PlaylistPanelInput::InsertTracksAt { pos, tracks } => {
                let objects = Self::to_objects(&tracks);
                let at = insert_tracks_position(pos, self.store.n_items());
                self.store.splice(at, 0, &objects);
                self.refresh_empty_state(&sender);
                self.update_header();
            }
            PlaylistPanelInput::RemoveRow(index) => {
                if should_remove_queue_row(index, self.store.n_items()) {
                    self.store.remove(index as u32);
                }
                self.refresh_empty_state(&sender);
                self.update_header();
            }
            PlaylistPanelInput::SetPlaying { idx } => {
                *self.current_track_identity.borrow_mut() =
                    idx.and_then(|i| self.store.item(i as u32));
                self.relight();
            }
            PlaylistPanelInput::ScrollToCurrent { idx } => self.scroll_to(idx),
            PlaylistPanelInput::SetPlaylistName(name) => {
                self.playlist_name = name;
                self.update_header();
            }
            PlaylistPanelInput::SetLibraryHasItems(has_items) => {
                self.library_has_items = has_items;
                // Refresh only matters while the placeholder is visible.
                if should_refresh_empty_placeholder(self.store.n_items()) {
                    self.refresh_empty_state(&sender);
                }
            }
            PlaylistPanelInput::ApplyColumnWidths(spec) => {
                // Suppress the resize watcher so restored widths aren't echoed
                // straight back to the shell as a persist request.
                self.applying_widths.set(true);
                apply_column_widths(&self.column_view, &spec);
                self.applying_widths.set(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        insert_tracks_position, is_current_indicator_identity_match, playlist_header_label,
        scroll_target_position, should_refresh_empty_placeholder, should_remove_queue_row,
    };
    #[cfg(not(miri))]
    use super::{is_current_indicator_track, playlist_track_objects};
    #[cfg(not(miri))]
    use crate::db::models::Track;
    #[cfg(not(miri))]
    use gtk::glib;
    #[cfg(not(miri))]
    use gtk::prelude::Cast;

    #[cfg(not(miri))]
    fn track(path: &str, title: &str, duration_secs: f64) -> Track {
        let mut track = Track::new_external_path_with_title(path, Some(title.to_string()));
        track.duration_secs = duration_secs;
        track
    }

    #[cfg(not(miri))]
    #[test]
    fn playlist_track_objects_preserve_ordered_tracks() {
        let tracks = [
            track("/music/one.flac", "One", 60.0),
            track("/music/two.flac", "Two", 120.0),
        ];

        let objects = playlist_track_objects(&tracks);
        let file_paths: Vec<String> = objects
            .iter()
            .map(|track_object| {
                track_object
                    .downcast_ref::<glib::BoxedAnyObject>()
                    .expect("playlist track object")
                    .borrow::<Track>()
                    .file_path
                    .clone()
            })
            .collect();

        assert_eq!(file_paths, ["/music/one.flac", "/music/two.flac"]);
    }

    #[test]
    fn current_indicator_identity_match_requires_current_same_identity() {
        assert!(is_current_indicator_identity_match(true, true));
        assert!(!is_current_indicator_identity_match(true, false));
        assert!(!is_current_indicator_identity_match(false, true));
        assert!(!is_current_indicator_identity_match(false, false));
    }

    #[cfg(not(miri))]
    #[test]
    fn current_indicator_matches_exact_track_identity() {
        let current_track_identity: glib::Object =
            glib::BoxedAnyObject::new(track("/music/current.flac", "Current", 60.0)).upcast();
        let other_track_identity: glib::Object =
            glib::BoxedAnyObject::new(track("/music/other.flac", "Other", 60.0)).upcast();

        assert!(is_current_indicator_track(
            Some(&current_track_identity),
            &current_track_identity
        ));
        assert!(!is_current_indicator_track(
            Some(&current_track_identity),
            &other_track_identity
        ));
        assert!(!is_current_indicator_track(None, &current_track_identity));
    }

    #[test]
    fn scroll_target_position_uses_three_row_lookahead_and_bounds() {
        assert_eq!(scroll_target_position(0, 2), None);
        assert_eq!(scroll_target_position(10, 2), Some(5));
        assert_eq!(scroll_target_position(4, 2), Some(3));
    }

    #[test]
    fn insert_tracks_position_clamps_to_existing_queue_len() {
        assert_eq!(insert_tracks_position(0, 3), 0);
        assert_eq!(insert_tracks_position(2, 3), 2);
        assert_eq!(insert_tracks_position(9, 3), 3);
    }

    #[test]
    fn empty_placeholder_refreshes_only_for_empty_queue() {
        assert!(should_refresh_empty_placeholder(0));
        assert!(!should_refresh_empty_placeholder(1));
    }

    #[test]
    fn remove_queue_row_accepts_only_existing_indices() {
        assert!(should_remove_queue_row(0, 1));
        assert!(should_remove_queue_row(2, 3));
        assert!(!should_remove_queue_row(3, 3));
    }

    #[test]
    fn playlist_header_label_uses_name_or_track_summary() {
        assert_eq!(playlist_header_label(0, 0.0, ""), "Playlist");
        assert_eq!(playlist_header_label(0, 0.0, "Road Mix"), "Road Mix");

        let one_track = playlist_header_label(1, 65.0, "Road Mix");
        assert!(one_track.starts_with("1 track · "));

        let two_tracks = playlist_header_label(2, 125.0, "Road Mix");
        assert!(two_tracks.starts_with("2 tracks · "));
    }
}
