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

use crate::audio::ui::{playlist_chrome, playlist_columns, playlist_interactions};
use crate::db::models::Track;
use crate::i18n::i18n;
use crate::shared::ui_helpers::format_duration_hm;

/// Live now-playing indicator widgets for one bound row, kept so `SetPlaying`
/// can re-light rows by object identity without rebinding the factory.
pub(super) type RowIndicator = (glib::Object, gtk::Image, gtk::Label);

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
        tracks
            .iter()
            .map(|t| glib::BoxedAnyObject::new(t.clone()).upcast())
            .collect()
    }

    /// Re-light the now-playing indicator on every bound row by object identity.
    fn relight(&self) {
        let current_track_identity = self.current_track_identity.borrow();
        for (track_identity, icon, label) in self.indicators.borrow().iter() {
            let is_current = current_track_identity.as_ref() == Some(track_identity);
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
        if count == 0 {
            return;
        }
        if let Some(selection) = self
            .column_view
            .model()
            .and_downcast::<gtk::MultiSelection>()
        {
            selection.select_item(idx as u32, true);
        }
        let cv = self.column_view.clone();
        let target = (idx + 3).min(count.saturating_sub(1)) as u32;
        glib::idle_add_local_once(move || {
            cv.scroll_to(target, None, gtk::ListScrollFlags::FOCUS, None);
        });
    }

    /// Refresh the toolbar header: the playlist name (or "Playlist") when the
    /// queue is empty, otherwise "N tracks · total duration".
    fn update_header(&self) {
        let count = self.store.n_items();
        if count == 0 {
            let title = if self.playlist_name.is_empty() {
                i18n("Playlist")
            } else {
                self.playlist_name.clone()
            };
            self.header_label.set_label(&title);
            return;
        }
        let total_secs: f64 = (0..count)
            .filter_map(|i| self.store.item(i).and_downcast::<glib::BoxedAnyObject>())
            .map(|boxed_track| boxed_track.borrow::<Track>().duration_secs)
            .sum();
        let queue_summary = format!(
            "{} {} · {}",
            count,
            if count == 1 {
                i18n("track")
            } else {
                i18n("tracks")
            },
            format_duration_hm(total_secs),
        );
        self.header_label.set_label(&queue_summary);
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
        adw::ToolbarView::new()
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
                let at = (pos as u32).min(self.store.n_items());
                self.store.splice(at, 0, &objects);
                self.refresh_empty_state(&sender);
                self.update_header();
            }
            PlaylistPanelInput::RemoveRow(index) => {
                if (index as u32) < self.store.n_items() {
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
                if self.store.n_items() == 0 {
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
