//! App-local Relm4 library panel (push-model).
//!
//! Single-column layout (`gtk::Stack`): a "browse"
//! page (grouping/search bar + library tree) and a "detail" page (back bar +
//! track `ColumnView`) swap in place — selecting a node shows the detail, the
//! back button returns to the tree. The panel NEVER touches the DB — it emits
//! intents and the shell pushes query results back.
//!
//! `PopulateDetail` splices the detail store; row activation emits `PlayTrack`.
//! The track list is the drawn `track_list` view shared with the queue: a
//! heading sorts the list, stars and the add button emit intents, and covers
//! are panel-owned (LRU + off-thread fill). Tree, grouping and dialogs are
//! shell-mediated.

use std::cell::RefCell;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use relm4::{Component, ComponentParts, ComponentSender};

use crate::audio::library_grouping::LibraryGrouping;
use crate::audio::ui::library_tree::{self, LibraryTreeHandle, LibraryTreeSnapshot};
use crate::audio::ui::track_list::{self, Behaviour, Columns, ListEvent, TrackList, track_of};
use crate::audio::ui::{
    library_detail_interactions, library_dialogs, library_grouping_ui, library_io,
    library_tag_editor,
};
use crate::db::models::{Track, WatchFolder};
use crate::i18n::i18n;

/// Covers drawn so far, by art hash; the size they are decoded at.
type ArtCache = Rc<RefCell<lru::LruCache<String, gdk::Texture>>>;
const COVER_EDGE: i32 = 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailContentState {
    EmptyPlaceholder,
    KeepDetailView,
    ReattachDetailView,
}

/// Wrap tracks as the `BoxedAnyObject`s the store/factories expect.
fn detail_track_objects(tracks: &[Track]) -> Vec<glib::Object> {
    tracks
        .iter()
        .map(|track| glib::BoxedAnyObject::new(track.clone()).upcast())
        .collect()
}

/// Snapshot detail tracks from a `gio::ListStore`-compatible item provider.
fn collect_tracks_from_boxed_store(
    item_count: u32,
    mut object_at: impl FnMut(u32) -> Option<glib::Object>,
) -> Vec<Track> {
    collect_tracks_from_tracks(item_count, |index| {
        object_at(index)
            .and_downcast::<glib::BoxedAnyObject>()
            .map(|boxed_track| boxed_track.borrow::<Track>().clone())
    })
}

fn collect_tracks_from_tracks(
    item_count: u32,
    mut track_at: impl FnMut(u32) -> Option<Track>,
) -> Vec<Track> {
    (0..item_count).filter_map(&mut track_at).collect()
}

/// DB ids for detail tracks from a `gio::ListStore`-compatible item provider.
fn collect_track_ids_from_boxed_store(
    item_count: u32,
    object_at: impl FnMut(u32) -> Option<glib::Object>,
) -> Vec<i64> {
    collect_track_ids_from_tracks(collect_tracks_from_boxed_store(item_count, object_at))
}

fn collect_track_ids_from_tracks(tracks: Vec<Track>) -> Vec<i64> {
    tracks.into_iter().map(|track| track.id).collect()
}

fn is_current_detail_indicator(current_path: Option<&str>, indicator_path: &str) -> bool {
    current_path == Some(indicator_path)
}

fn detail_content_state(row_count: u32, is_detail_view_attached: bool) -> DetailContentState {
    if row_count == 0 {
        DetailContentState::EmptyPlaceholder
    } else if is_detail_view_attached {
        DetailContentState::KeepDetailView
    } else {
        DetailContentState::ReattachDetailView
    }
}

fn should_clear_tag_editor_handle(
    editor_track_id: i64,
    resolved_track_id: i64,
    was_result_applied: bool,
) -> bool {
    editor_track_id == resolved_track_id && !was_result_applied
}

/// Messages the shell pushes into the library panel.
#[derive(Debug)]
pub enum LibraryPanelInput {
    /// Replace the detail track list after the shell resolves a selected node.
    /// Splices the store; never rebuilds the view.
    PopulateDetail(Vec<Track>),
    /// The currently-playing file path (or `None`) — re-lights the detail
    /// now-playing indicator by file-path match.
    SetPlayingPath(Option<String>),
    /// Restore saved detail column widths (`title=width,…`), pushed by the shell
    /// at construction. Suppresses the persist echo while applying.
    ApplyColumnWidths(String),
    /// Replace the whole left-pane tree (shell ran the group + atomic preload).
    SetLibraryTree(LibraryTreeSnapshot),
    /// Sync the grouping dropdown to the shell's restored preset (boot; no
    /// re-emit of `GroupingChanged`).
    SetGrouping(LibraryGrouping),
    /// Open the tag editor for a track (shell resolves the full track first).
    EditTags(Track),
    /// Push the MusicBrainz lookup result into the currently open tag editor.
    MusicBrainzLookupResolved {
        /// Track DB id the lookup was requested for.
        track_id: i64,
        /// Returned MusicBrainz fields, or `None` if no confident match exists.
        metadata: Option<crate::library::musicbrainz::MbMetadata>,
    },
    /// Open the add-to-playlist picker (shell resolved the playlist list).
    ShowAddToPlaylistPicker {
        /// Track to add.
        track: Track,
        /// `(id, name)` of existing playlists.
        playlists: Vec<(i64, String)>,
    },
    /// Open the watch-folder manager (shell resolved the folder list).
    ShowManageFolders(Vec<WatchFolder>),
    /// Export the current detail tracks as an m3u8 (panel picks the path).
    ExportDetailM3u8,
    /// Import a playlist file into the library (panel picks the file).
    ImportPlaylist,
    /// Save the current detail tracks as a new playlist.
    SaveDetailAsPlaylist,
}

/// Intents the library panel emits for the shell to act on.
#[derive(Debug)]
pub enum LibraryPanelOutput {
    /// Play a track activated in the detail view (shell adds it to the queue +
    /// plays if idle).
    PlayTrack(Track),
    /// Context-menu "Play": replace the queue with these tracks (the detail
    /// list, in visual order) and jump to `index`.
    PlayAllFrom {
        /// Tracks in visual order.
        tracks: Vec<Track>,
        /// Index to start playback at.
        index: usize,
    },
    /// Insert a track right after the current one (Play Next).
    EnqueueNext(Track),
    /// Append a track to the queue (Add to Queue).
    EnqueueTrack(Track),
    /// Persist a new rating for a track (1–5 stars).
    RateTrack {
        /// Track DB id.
        id: i64,
        /// New rating 1–5.
        rating: i32,
    },
    /// Show the "add to playlist" dialog for a track (shell owns the dialog).
    ShowAddToPlaylist(Track),
    /// Reveal a file in the desktop file manager.
    RevealInFileManager(String),
    /// Show the track-info dialog (shell owns the dialog + metadata).
    ShowTrackInfo(Track),
    /// Persist the detail column widths (`title=width,…`) — the shell writes the
    /// settings key. Debounced to one emission per resize gesture.
    PersistColumnWidths(String),
    /// A library tree node was activated — shell resolves it → `PopulateDetail`.
    NodeSelected {
        /// `BrowserNodeKind as u32`.
        kind: u32,
        /// DB filter value.
        key: String,
        /// Tab-joined secondary payload (leaf metadata / album-artist).
        secondary: String,
        /// `TreeListRow::depth()`.
        depth: u32,
    },
    /// Grouping preset changed — shell re-runs the tree query.
    GroupingChanged(LibraryGrouping),
    /// Age-filter dropdown index changed.
    AgeFilterChanged(u32),
    /// Debounced library search text changed.
    SearchTextChanged(String),
    /// Write edited tags to a track (shell: off-thread file write + DB update).
    WriteTags {
        /// Track DB id.
        track_id: i64,
        /// Audio file path to write the tags into.
        file_path: String,
        /// Edited field values.
        fields: library_tag_editor::TagEditFields,
    },
    /// MusicBrainz lookup request (advisory; fills empty fields only).
    MusicBrainzLookup(Track),
    /// User-facing notice text.
    Notify(String),
    /// Import an m3u/m3u8/pls file (shell parses + inserts).
    ImportPlaylistFile {
        /// Picked file path.
        path: PathBuf,
    },
    /// Save tracks as a new playlist.
    SaveTracksAsPlaylist {
        /// New playlist name.
        name: String,
        /// Track ids to add.
        track_ids: Vec<i64>,
    },
    /// Add a watched folder + scan it.
    AddWatchFolder {
        /// Folder path.
        path: PathBuf,
    },
    /// Stop monitoring a folder; retain its tracks until explicit review.
    RemoveWatchFolder {
        /// Folder DB id.
        id: i64,
    },
    /// Review missing records without deleting files from disk.
    ReviewUnavailable,
    /// Rescan all watched folders.
    ScanNow,
    /// Add a track to an existing playlist.
    AddTrackToPlaylist {
        /// Track DB id.
        track_id: i64,
        /// Playlist DB id.
        playlist_id: i64,
    },
    /// Create a new playlist seeded with one track.
    CreatePlaylistWithTrack {
        /// New playlist name.
        name: String,
        /// Track DB id.
        track_id: i64,
    },
}

/// Relm4 library panel component (left tree | right detail).
pub struct LibraryPanel {
    detail_store: gio::ListStore,
    detail: TrackList,
    /// Right-pane container; swapped between the track list and an
    /// empty-state placeholder (the view is reattached, never rebuilt).
    content_scroll: gtk::ScrolledWindow,
    /// Currently-playing file path (emphasis target).
    current_path: Rc<RefCell<Option<String>>>,
    /// Handle to push tree data after construction (atomic splice, no rebuild).
    library_tree_handle: LibraryTreeHandle,
    /// Grouping dropdown + signal id, synced to the restored preset on boot.
    grouping_controls: library_grouping_ui::GroupingControls,
    /// Weak handle to the currently open tag editor, used for async lookup
    /// results.
    tag_editor: Rc<RefCell<Option<library_tag_editor::TagEditorHandle>>>,
}

impl LibraryPanel {
    /// Show the detail `ColumnView` when it has rows, otherwise an empty-state
    /// placeholder. The view is reattached, never rebuilt.
    fn refresh_detail_empty_state(&self) {
        let is_detail_view_attached = self
            .content_scroll
            .child()
            .as_ref()
            .is_some_and(|child| child == self.detail.view.upcast_ref::<gtk::Widget>());

        self.detail
            .header
            .set_visible(self.detail_store.n_items() > 0);
        match detail_content_state(self.detail_store.n_items(), is_detail_view_attached) {
            DetailContentState::EmptyPlaceholder => {
                let empty = adw::StatusPage::builder()
                    .icon_name("folder-music-symbolic")
                    .title(i18n("No Tracks"))
                    .description(i18n("Select an item on the left to view its tracks"))
                    .vexpand(true)
                    .build();
                self.content_scroll.set_child(Some(&empty));
            }
            DetailContentState::KeepDetailView => {}
            DetailContentState::ReattachDetailView => {
                self.content_scroll.set_child(Some(&self.detail.view));
            }
        }
    }
}

/// Library behaviour: the playing file emphasised, covers decoded off the main
/// loop, and the selection dragged as file paths (for the queue or outside).
fn library_behaviour(
    selection: &gtk::MultiSelection,
    current_path: &Rc<RefCell<Option<String>>>,
    shown: &Rc<RefCell<glib::WeakRef<big_item_view::ItemView>>>,
) -> Behaviour {
    let current = Rc::clone(current_path);
    let cache: ArtCache = Rc::new(RefCell::new(lru::LruCache::new(
        NonZeroUsize::new(200).unwrap_or(NonZeroUsize::MIN),
    )));
    let pending: Rc<RefCell<HashSet<String>>> = Rc::default();
    let shown = Rc::clone(shown);
    let selection = selection.downgrade();
    Behaviour {
        is_current: Box::new(move |_, track| {
            is_current_detail_indicator(current.borrow().as_deref(), &track.file_path)
        }),
        cover: Some(Box::new(move |track| {
            let hash = track.art_hash.clone().filter(|hash| !hash.is_empty())?;
            if let Some(texture) = cache.borrow_mut().get(&hash) {
                return Some(texture.clone());
            }
            // One decode per cover, however often the row is drawn meanwhile.
            if pending.borrow_mut().insert(hash.clone()) {
                let path = track.file_path.clone();
                let (cache, pending, shown) =
                    (Rc::clone(&cache), Rc::clone(&pending), Rc::clone(&shown));
                big_media_components::player_chrome::spawn_blocking_result(
                    move || {
                        let data = crate::library::metadata::extract_album_art(&path)?;
                        big_media_components::media::images::bytes_to_thumbnail(&data, COVER_EDGE)
                    },
                    move |texture| {
                        pending.borrow_mut().remove(&hash);
                        if let Some(texture) = texture {
                            cache.borrow_mut().put(hash, texture);
                            if let Some(view) = shown.borrow().upgrade() {
                                view.refresh_all();
                            }
                        }
                    },
                );
            }
            None
        })),
        drag: Some(Box::new(move |positions| {
            let selection = selection.upgrade()?;
            let paths: Vec<String> = positions
                .iter()
                .filter_map(|position| selection.item(*position))
                .filter_map(|item| track_of(&item).map(|track| track.file_path.clone()))
                .collect();
            (!paths.is_empty())
                .then(|| gdk::ContentProvider::for_value(&paths.join("\n").to_value()))
        })),
        drop: None,
    }
}

/// A heading, a star or a row's button pressed in the library list.
fn on_list_event(store: &gio::ListStore, event: ListEvent, out: &ComponentSender<LibraryPanel>) {
    match event {
        ListEvent::Sorted(column, way) => store.sort(|a, b| {
            let order = match (track_of(a), track_of(b)) {
                (Some(a), Some(b)) => column.compare(&a, &b),
                _ => std::cmp::Ordering::Equal,
            };
            if way == gtk::SortType::Descending {
                order.reverse()
            } else {
                order
            }
        }),
        ListEvent::Widths(spec) => {
            let _ = out.output(LibraryPanelOutput::PersistColumnWidths(spec));
        }
        ListEvent::Rate(position, rating) => {
            if let Some(id) = track_list::rate_in_place(store, position, rating) {
                let _ = out.output(LibraryPanelOutput::RateTrack { id, rating });
            }
        }
        ListEvent::Action(position) => {
            if let Some(track) = store
                .item(position)
                .and_then(|item| track_of(&item).map(|t| t.clone()))
            {
                let _ = out.output(LibraryPanelOutput::ShowAddToPlaylist(track));
            }
        }
    }
}

impl Component for LibraryPanel {
    type Init = ();
    type Input = LibraryPanelInput;
    type Output = LibraryPanelOutput;
    type CommandOutput = ();
    type Root = gtk::Stack;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::SlideLeftRight)
            .transition_duration(150)
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Browse page: grouping/age/search bar + library tree.
        let left = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let (grouping_bar, grouping_controls) = library_grouping_ui::build_grouping_bar(&sender);
        left.append(&grouping_bar);
        let (tree_view, library_tree_handle) = library_tree::build_library_tree(&sender);
        let tree_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&tree_view)
            .build();
        left.append(&tree_scroll);
        let tag_editor: Rc<RefCell<Option<library_tag_editor::TagEditorHandle>>> =
            Rc::new(RefCell::new(None));

        // Detail track list (right pane).
        let detail_store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::MultiSelection::new(Some(detail_store.clone()));
        let current_path: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let shown: Rc<RefCell<glib::WeakRef<big_item_view::ItemView>>> = Rc::default();
        let detail = {
            let behaviour = library_behaviour(&selection, &current_path, &shown);
            let (store, out) = (detail_store.downgrade(), sender.clone());
            TrackList::new(
                Columns::library(),
                behaviour,
                &i18n("Tracks"),
                move |event| {
                    if let Some(store) = store.upgrade() {
                        on_list_event(&store, event, &out);
                    }
                },
            )
        };
        shown.replace(detail.view.downgrade());
        detail.view.set_model(Some(&selection));
        detail.view.add_css_class("library-detail-view");

        // Row activation → PlayTrack.
        {
            let (store, out) = (detail_store.downgrade(), sender.clone());
            detail.view.connect_activate(move |_, position| {
                if let Some(track) = store
                    .upgrade()
                    .and_then(|store| store.item(position))
                    .and_then(|item| track_of(&item).map(|track| track.clone()))
                {
                    let _ = out.output(LibraryPanelOutput::PlayTrack(track));
                }
            });
        }
        {
            let (store, out) = (detail_store.downgrade(), sender.clone());
            detail
                .view
                .connect_context_menu(move |view, position, x, y| {
                    let Some(store) = store.upgrade() else {
                        return;
                    };
                    let tracks = library_detail_interactions::gather_visual_tracks(&store);
                    if let Some(index) =
                        library_detail_interactions::detail_context_index(tracks.len(), position)
                    {
                        library_detail_interactions::show_detail_context_menu(
                            view, x, y, tracks, index, &out,
                        );
                    }
                });
        }

        let content_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .child(&detail.view)
            .build();

        // Single-column layout: the browse tree and the
        // selected-node track list swap in place inside a Stack. Selecting a node
        // shows the detail page; the back button returns to the tree.
        let detail_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let back_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_start(6)
            .margin_end(6)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        let back_button = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .css_classes(["flat"])
            .build();
        back_button.set_tooltip_text(Some(&i18n("Back to library")));
        back_button.update_property(&[gtk::accessible::Property::Label(&i18n("Back to library"))]);
        // Weak: back_button lives INSIDE root — a strong capture is a
        // closure⇄widget self-cycle pinning the whole library subtree.
        let stack_for_back = root.downgrade();
        back_button.connect_clicked(move |_| {
            if let Some(stack) = stack_for_back.upgrade() {
                stack.set_visible_child_name("browse");
            }
        });
        back_bar.append(&back_button);
        detail_page.append(&back_bar);
        detail_page.append(&detail.page(&content_scroll));

        root.add_named(&left, Some("browse"));
        root.add_named(&detail_page, Some("detail"));
        root.set_visible_child_name("browse");

        let model = Self {
            detail_store,
            detail,
            content_scroll,
            current_path,
            library_tree_handle,
            grouping_controls,
            tag_editor,
        };
        model.refresh_detail_empty_state();

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        let parent = root.root().and_downcast::<gtk::Window>();
        match message {
            LibraryPanelInput::PopulateDetail(tracks) => {
                let boxed_track_objects = detail_track_objects(&tracks);
                self.detail_store
                    .splice(0, self.detail_store.n_items(), &boxed_track_objects);
                self.refresh_detail_empty_state();
                // Selecting a node swaps the single column to the track list.
                root.set_visible_child_name("detail");
            }
            LibraryPanelInput::SetPlayingPath(path) => {
                *self.current_path.borrow_mut() = path;
                self.detail.view.refresh_all();
            }
            LibraryPanelInput::ApplyColumnWidths(spec) => self.detail.apply_widths(&spec),
            LibraryPanelInput::SetLibraryTree(data) => self.library_tree_handle.set_tree(data),
            LibraryPanelInput::SetGrouping(grouping) => {
                library_grouping_ui::select_grouping(&self.grouping_controls, &grouping);
            }
            LibraryPanelInput::EditTags(track) => {
                *self.tag_editor.borrow_mut() =
                    library_tag_editor::show_tag_editor(parent.as_ref(), &track, &sender);
            }
            LibraryPanelInput::MusicBrainzLookupResolved { track_id, metadata } => {
                let should_clear = self.tag_editor.borrow().as_ref().is_some_and(|editor| {
                    let editor_track_id = editor.track_id();
                    let was_result_applied = editor.apply_musicbrainz_result(metadata);
                    should_clear_tag_editor_handle(editor_track_id, track_id, was_result_applied)
                });
                if should_clear {
                    self.tag_editor.borrow_mut().take();
                }
            }
            LibraryPanelInput::ShowAddToPlaylistPicker { track, playlists } => {
                if let Some(win) = parent.as_ref() {
                    library_dialogs::show_add_to_playlist(win, &track, &playlists, &sender);
                }
            }
            LibraryPanelInput::ShowManageFolders(folders) => {
                if let Some(win) = parent.as_ref() {
                    library_io::show_manage_folders(win, &folders, &sender);
                }
            }
            LibraryPanelInput::ExportDetailM3u8 => {
                if let Some(win) = parent.as_ref() {
                    let tracks =
                        collect_tracks_from_boxed_store(self.detail_store.n_items(), |index| {
                            self.detail_store.item(index)
                        });
                    library_io::export_detail_m3u8(win, tracks, &sender);
                }
            }
            LibraryPanelInput::ImportPlaylist => {
                if let Some(win) = parent.as_ref() {
                    library_io::import_playlist(win, &sender);
                }
            }
            LibraryPanelInput::SaveDetailAsPlaylist => {
                if let Some(win) = parent.as_ref() {
                    let track_ids =
                        collect_track_ids_from_boxed_store(self.detail_store.n_items(), |index| {
                            self.detail_store.item(index)
                        });
                    library_io::save_tracks_as_playlist(win, track_ids, &sender);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track_with_id(id: i64, title: &str) -> Track {
        let mut track = Track::new_external_path_with_title(
            format!("/music/{title}.flac"),
            Some(title.to_string()),
        );
        track.id = id;
        track
    }

    #[cfg(not(miri))]
    fn boxed_track(track: Track) -> glib::Object {
        glib::BoxedAnyObject::new(track).upcast()
    }

    #[cfg(not(miri))]
    #[test]
    fn detail_track_objects_preserve_track_order() {
        let tracks = [track_with_id(7, "one"), track_with_id(9, "two")];

        let boxed_track_objects = detail_track_objects(&tracks);
        let file_paths: Vec<String> = boxed_track_objects
            .iter()
            .map(|object| {
                object
                    .clone()
                    .downcast::<glib::BoxedAnyObject>()
                    .expect("detail track object")
                    .borrow::<Track>()
                    .file_path
                    .clone()
            })
            .collect();

        assert_eq!(file_paths, ["/music/one.flac", "/music/two.flac"]);
    }

    #[cfg(not(miri))]
    #[test]
    fn collect_tracks_from_boxed_store_preserves_existing_items_only() {
        let boxed_track_objects = [
            Some(boxed_track(track_with_id(7, "one"))),
            None,
            Some(boxed_track(track_with_id(9, "two"))),
        ];

        let file_paths: Vec<String> =
            collect_tracks_from_boxed_store(boxed_track_objects.len() as u32, |index| {
                boxed_track_objects
                    .get(index as usize)
                    .and_then(Clone::clone)
            })
            .into_iter()
            .map(|track| track.file_path)
            .collect();

        assert_eq!(file_paths, ["/music/one.flac", "/music/two.flac"]);
    }

    #[test]
    fn collect_tracks_from_tracks_preserves_existing_items_only() {
        let tracks = [
            Some(track_with_id(7, "one")),
            None,
            Some(track_with_id(9, "two")),
        ];

        let file_paths: Vec<String> = collect_tracks_from_tracks(tracks.len() as u32, |index| {
            tracks.get(index as usize).cloned().flatten()
        })
        .into_iter()
        .map(|track| track.file_path)
        .collect();

        assert_eq!(file_paths, ["/music/one.flac", "/music/two.flac"]);
    }

    #[cfg(not(miri))]
    #[test]
    fn collect_track_ids_from_boxed_store_returns_database_ids() {
        let boxed_track_objects = [
            Some(boxed_track(track_with_id(7, "one"))),
            Some(boxed_track(track_with_id(9, "two"))),
        ];

        assert_eq!(
            collect_track_ids_from_boxed_store(boxed_track_objects.len() as u32, |index| {
                boxed_track_objects
                    .get(index as usize)
                    .and_then(Clone::clone)
            }),
            [7, 9]
        );
    }

    #[test]
    fn collect_track_ids_from_tracks_returns_database_ids() {
        let tracks = vec![track_with_id(7, "one"), track_with_id(9, "two")];

        assert_eq!(collect_track_ids_from_tracks(tracks), [7, 9]);
    }

    #[test]
    fn detail_indicator_matches_exact_current_path_only() {
        assert!(is_current_detail_indicator(
            Some("/music/one.flac"),
            "/music/one.flac"
        ));
        assert!(!is_current_detail_indicator(
            Some("/music/two.flac"),
            "/music/one.flac"
        ));
        assert!(!is_current_detail_indicator(None, "/music/one.flac"));
    }

    #[test]
    fn detail_content_state_distinguishes_empty_attached_and_missing_view() {
        assert_eq!(
            detail_content_state(0, false),
            DetailContentState::EmptyPlaceholder
        );
        assert_eq!(
            detail_content_state(3, true),
            DetailContentState::KeepDetailView
        );
        assert_eq!(
            detail_content_state(3, false),
            DetailContentState::ReattachDetailView
        );
    }

    #[test]
    fn musicbrainz_editor_handle_is_cleared_only_for_matching_stale_editor() {
        assert!(should_clear_tag_editor_handle(42, 42, false));
        assert!(!should_clear_tag_editor_handle(42, 7, false));
        assert!(!should_clear_tag_editor_handle(42, 42, true));
    }
}
