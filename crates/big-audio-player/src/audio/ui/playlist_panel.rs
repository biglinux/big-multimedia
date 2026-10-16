//! App-local Relm4 playlist/queue panel (push-model).
//!
//! The queue is shell-owned: this component renders `SetQueue`/`SetPlaying`
//! snapshots and emits intents (`JumpTo`/`RemoveTracks`/`RateTrack`). Data
//! changes splice the backing `ListStore` in place, only where they differ.
//! The playing track matches by OBJECT IDENTITY so duplicate tracks in the
//! queue light independently.
//!
//! The rows are a drawn `track_list` view: a heading sorts the queue itself,
//! stars rate, the row's button removes, and a drag reorders or adds files.
//! Saved-playlist load, smart playlists, and import/export belong to the
//! LIBRARY panel; queue mutation + undo stay shell-side.

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use relm4::{Component, ComponentParts, ComponentSender};
use std::cell::RefCell;
use std::rc::Rc;

use big_relm4_components::layout::toolbar_pane::{BigToolbarPane, BigToolbarPaneSpec};

use crate::audio::ui::track_list::{Behaviour, Columns, ListEvent, TrackList, track_of};
use crate::audio::ui::{playlist_chrome, playlist_interactions};
use crate::db::models::Track;
use crate::i18n::i18n;
use big_media_components::time::duration_hm as format_duration_hm;

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

/// How many rows the store and a new snapshot share at the start and at the
/// end. `same(row, track)` compares one stored row with a snapshot track.
fn shared_ends(
    rows: usize,
    tracks: &[Track],
    same: impl Fn(usize, &Track) -> bool,
) -> (usize, usize) {
    let start = tracks
        .iter()
        .take(rows)
        .enumerate()
        .take_while(|(row, track)| same(*row, track))
        .count();
    let end = (1..=rows.min(tracks.len()) - start)
        .take_while(|&back| same(rows - back, &tracks[tracks.len() - back]))
        .count();
    (start, end)
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
    /// Select every row (Ctrl+A).
    SelectAll,
    /// Active saved-playlist name changed.
    SetPlaylistName(String),
    /// Whether the library has any tracks; picks the empty-queue placeholder
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
    /// User sorted by a column header: commit the visible order as the new
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
    /// Download podcast episodes as `(url, title)` pairs (HTTP entries only).
    DownloadEpisodes(Vec<(String, String)>),
    /// Delete tracks from disk (trash or permanent); shell does the fs delete,
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
    /// First-run "Open Files" picker chose audio files; shell imports + plays.
    OpenFiles(Vec<String>),
    /// Persist the queue column widths (`title=width,…`); the shell writes the
    /// settings key. Debounced to one emission per resize gesture.
    PersistColumnWidths(String),
}

/// Relm4 playlist/queue panel component.
pub struct PlaylistPanel {
    store: gio::ListStore,
    selection: gtk::MultiSelection,
    list: TrackList,
    /// Scroll area of the list; holds the empty-state widget instead while the
    /// queue is empty (the view is put back, never rebuilt).
    scroll: gtk::ScrolledWindow,
    /// Toolbar header label: playlist name when empty, "N tracks · duration"
    /// when the queue has items.
    header_label: gtk::Label,
    /// The currently-playing store object (identity target for the emphasis).
    current_track_identity: Rc<RefCell<Option<glib::Object>>>,
    /// Active saved-playlist name (shown in the toolbar header when the queue
    /// is empty).
    playlist_name: String,
    /// Library-has-tracks flag; picks the empty-queue placeholder variant.
    library_has_items: bool,
}

impl PlaylistPanel {
    /// Wrap queue tracks as the `BoxedAnyObject`s the store holds.
    fn to_objects(tracks: &[Track]) -> Vec<glib::Object> {
        playlist_track_objects(tracks)
    }

    /// Select the current row and scroll it into view, three rows ahead.
    fn scroll_to(&self, idx: usize) {
        let count = self.store.n_items() as usize;
        let Some(target) = scroll_target_position(count, idx) else {
            return;
        };
        self.selection.select_item(idx as u32, true);
        self.list.view.scroll_to(target, gtk::ListScrollFlags::NONE);
    }

    /// Refresh the toolbar header: the playlist name (or "Playlist") when the
    /// queue is empty, otherwise "N tracks · total duration".
    fn update_header(&self) {
        let count = self.store.n_items();
        let total_secs: f64 = (0..count)
            .filter_map(|i| self.store.item(i))
            .filter_map(|item| track_of(&item).map(|track| track.duration_secs))
            .sum();
        self.header_label.set_label(&playlist_header_label(
            count,
            total_secs,
            &self.playlist_name,
        ));
    }

    /// Show the empty-state placeholder when the queue is empty, otherwise
    /// (re)attach the list. The view is reattached, never rebuilt.
    fn refresh_empty_state(&self, sender: &ComponentSender<Self>) {
        let empty = self.store.n_items() == 0;
        self.list.header.set_visible(!empty);
        if empty {
            let placeholder = playlist_chrome::build_empty_state(self.library_has_items, sender);
            self.scroll.set_child(Some(&placeholder));
        } else {
            let view: gtk::Widget = self.list.view.clone().upcast();
            if self.scroll.child().as_ref() != Some(&view) {
                self.scroll.set_child(Some(&self.list.view));
            }
        }
    }

    /// `(queue index, track)` for every selected row.
    fn selected_tracks(
        store: &gio::ListStore,
        selection: &gtk::MultiSelection,
    ) -> Vec<(usize, Track)> {
        let bitset = selection.selection();
        (0..bitset.size())
            .map(|nth| bitset.nth(nth as u32))
            .filter_map(|position| {
                let item = store.item(position)?;
                Some((position as usize, track_of(&item)?.clone()))
            })
            .collect()
    }
}

/// Queue behaviour: identity emphasis, single-row reorder drags, and drops of
/// that row or of files from outside.
fn queue_behaviour(
    store: &gio::ListStore,
    selection: &gtk::MultiSelection,
    current: &Rc<RefCell<Option<glib::Object>>>,
    sender: &ComponentSender<PlaylistPanel>,
) -> Behaviour {
    let current = Rc::clone(current);
    let selection_weak = selection.downgrade();
    let (store_weak, out) = (store.downgrade(), sender.clone());
    Behaviour {
        is_current: Box::new(move |item, _| {
            is_current_indicator_track(current.borrow().as_ref(), item)
        }),
        cover: None,
        // Multi-row reorder is left out: where several rows land is ambiguous.
        drag: Some(Box::new(move |positions| {
            let selection = selection_weak.upgrade()?;
            let [position] = positions else {
                return None;
            };
            (selection.selection().size() == 1)
                .then(|| gdk::ContentProvider::for_value(&i64::from(*position).to_value()))
        })),
        drop: Some(Box::new(move |target, value| {
            let Some(store) = store_weak.upgrade() else {
                return false;
            };
            let count = store.n_items() as usize;
            if let Ok(from) = value.get::<i64>() {
                let Some(from) = playlist_interactions::valid_reorder_source_index(from) else {
                    return false;
                };
                let to = target.map_or(count.saturating_sub(1), |at| at as usize);
                let _ = out.output(PlaylistPanelOutput::ReorderTrack { from, to });
                return true;
            }
            let paths: Vec<String> = big_app_kit::widgets::drop_paths_from_value(value)
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            if paths.is_empty() {
                return false;
            }
            let insert_pos = target.map_or(count, |at| at as usize);
            let _ = out.output(PlaylistPanelOutput::DropPaths { insert_pos, paths });
            true
        })),
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
        let selection = gtk::MultiSelection::new(Some(store.clone()));
        let current_track_identity: Rc<RefCell<Option<glib::Object>>> = Rc::new(RefCell::new(None));

        let list = {
            let behaviour = queue_behaviour(&store, &selection, &current_track_identity, &sender);
            let (store, selection, out) =
                (store.downgrade(), selection.downgrade(), sender.clone());
            TrackList::new(
                Columns::queue(),
                behaviour,
                &i18n("Playlist"),
                move |event| {
                    let (Some(store), Some(selection)) = (store.upgrade(), selection.upgrade())
                    else {
                        return;
                    };
                    on_list_event(&store, &selection, event, &out);
                },
            )
        };
        list.view.set_model(Some(&selection));
        list.view.set_drop_types(
            &[
                i64::static_type(),
                gdk::FileList::static_type(),
                gio::File::static_type(),
                String::static_type(),
            ],
            gdk::DragAction::COPY | gdk::DragAction::MOVE,
        );
        list.view.add_css_class("playlist-view");

        // Row activation → JumpTo (shell loads + plays).
        {
            let out = sender.clone();
            list.view.connect_activate(move |_, position| {
                let _ = out.output(PlaylistPanelOutput::JumpTo {
                    index: position as usize,
                });
            });
        }
        playlist_interactions::wire_delete_key(&list.view, &selection, &sender);
        {
            let (store, selection, out) =
                (store.downgrade(), selection.downgrade(), sender.clone());
            list.view.connect_context_menu(move |view, position, x, y| {
                let (Some(store), Some(selection)) = (store.upgrade(), selection.upgrade()) else {
                    return;
                };
                // A press on an unselected row makes it the selection, so the
                // menu acts on what was pointed at.
                if position != gtk::INVALID_LIST_POSITION && !selection.is_selected(position) {
                    selection.select_item(position, true);
                }
                let selected = Self::selected_tracks(&store, &selection);
                if !selected.is_empty() {
                    playlist_interactions::show_track_context_menu(view, x, y, selected, &out);
                }
            });
        }

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&list.view)
            .build();
        let page = list.page(&scroll);

        let (toolbar, header_label) = playlist_chrome::build_toolbar(&sender);
        root.add_top_bar(&toolbar);
        root.set_content(Some(&page));

        let model = Self {
            store,
            selection,
            list,
            scroll,
            header_label,
            current_track_identity,
            playlist_name: String::new(),
            library_has_items: false,
        };
        // Empty queue at boot → show the placeholder until the shell pushes a
        // SetQueue (which reattaches the list).
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
                // Replace only the rows that changed: each metadata batch of a
                // large open sends the whole queue, and new objects for every
                // row made GTK destroy and rebuild every row widget each time.
                let rows = self.store.n_items() as usize;
                let (start, end) = shared_ends(rows, &tracks, |row, track| {
                    self.store
                        .item(row as u32)
                        .is_some_and(|item| track_of(&item).is_some_and(|stored| *stored == *track))
                });
                let objects = Self::to_objects(&tracks[start..tracks.len() - end]);
                let removed = rows - start - end;
                if removed > 0 || !objects.is_empty() {
                    self.store.splice(start as u32, removed as u32, &objects);
                }
                *self.current_track_identity.borrow_mut() =
                    current_idx.and_then(|i| self.store.item(i as u32));
                self.refresh_empty_state(&sender);
                self.update_header();
                self.list.view.refresh_all();
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
                self.list.view.refresh_all();
            }
            PlaylistPanelInput::ScrollToCurrent { idx } => self.scroll_to(idx),
            PlaylistPanelInput::SelectAll => {
                self.selection.select_all();
            }
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
            PlaylistPanelInput::ApplyColumnWidths(spec) => self.list.apply_widths(&spec),
        }
    }
}

/// A heading, a star or a row's button pressed in the queue.
fn on_list_event(
    store: &gio::ListStore,
    selection: &gtk::MultiSelection,
    event: ListEvent,
    out: &ComponentSender<PlaylistPanel>,
) {
    match event {
        // The queue takes the order the heading asks for.
        ListEvent::Sorted(column, way) => {
            let mut tracks: Vec<Track> = (0..store.n_items())
                .filter_map(|i| store.item(i))
                .filter_map(|item| track_of(&item).map(|track| track.clone()))
                .collect();
            tracks.sort_by(|a, b| {
                let order = column.compare(a, b);
                if way == gtk::SortType::Descending {
                    order.reverse()
                } else {
                    order
                }
            });
            if !tracks.is_empty() {
                let _ = out.output(PlaylistPanelOutput::CommitSortedOrder { tracks });
            }
        }
        ListEvent::Widths(spec) => {
            let _ = out.output(PlaylistPanelOutput::PersistColumnWidths(spec));
        }
        ListEvent::Rate(position, rating) => {
            if let Some(id) = crate::audio::ui::track_list::rate_in_place(store, position, rating) {
                let _ = out.output(PlaylistPanelOutput::RateTrack { id, rating });
            }
        }
        ListEvent::Action(position) => {
            selection.unselect_all();
            let _ = out.output(PlaylistPanelOutput::RemoveTracks {
                indices: vec![position as usize],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        insert_tracks_position, is_current_indicator_identity_match, playlist_header_label,
        scroll_target_position, shared_ends, should_refresh_empty_placeholder,
        should_remove_queue_row,
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
    fn indicator_needs_current_identity() {
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
    fn scroll_target_looks_three_rows_ahead() {
        assert_eq!(scroll_target_position(0, 2), None);
        assert_eq!(scroll_target_position(10, 2), Some(5));
        assert_eq!(scroll_target_position(4, 2), Some(3));
    }

    #[test]
    fn insert_position_clamps_to_queue() {
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
    fn header_label_uses_name_or_summary() {
        assert_eq!(playlist_header_label(0, 0.0, ""), "Playlist");
        assert_eq!(playlist_header_label(0, 0.0, "Road Mix"), "Road Mix");

        let one_track = playlist_header_label(1, 65.0, "Road Mix");
        assert!(one_track.starts_with("1 track · "));

        let two_tracks = playlist_header_label(2, 125.0, "Road Mix");
        assert!(two_tracks.starts_with("2 tracks · "));
    }

    /// Metadata for rows 1 and 2, a removal, an insertion, duplicates and
    /// empty queues: only the rows between the shared ends are replaced.
    #[cfg(not(miri))]
    #[test]
    fn shared_ends_isolate_changed_rows() {
        let named = |names: &[&str]| -> Vec<crate::db::models::Track> {
            names.iter().map(|name| track(name, name, 1.0)).collect()
        };
        let ends = |old: &[&str], new: &[&str]| {
            let (old, new) = (named(old), named(new));
            shared_ends(old.len(), &new, |row, track| old[row] == *track)
        };
        let mut retitled = named(&["a", "b", "c", "d"]);
        retitled[1].title = Some("B".into());
        retitled[2].title = Some("C".into());
        let old = named(&["a", "b", "c", "d"]);
        assert_eq!(
            shared_ends(4, &retitled, |row, track| old[row] == *track),
            (1, 1)
        );
        assert_eq!(ends(&["a", "b", "c"], &["a", "b", "c"]), (3, 0));
        assert_eq!(ends(&["a", "b", "c"], &["a", "c"]), (1, 1));
        assert_eq!(ends(&["a", "c"], &["a", "b", "c"]), (1, 1));
        assert_eq!(ends(&["a", "a"], &["a", "a", "a"]), (2, 0));
        assert_eq!(ends(&["a", "b"], &["c", "d"]), (0, 0));
        assert_eq!(ends(&[], &["a"]), (0, 0));
        assert_eq!(ends(&["a"], &[]), (0, 0));
    }
}
