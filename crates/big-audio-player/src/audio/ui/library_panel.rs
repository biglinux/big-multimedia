//! App-local Relm4 library panel (push-model) — port in progress.
//!
//! Single-column layout (`gtk::Stack`, mirroring the god-object): a "browse"
//! page (grouping/search bar + library tree) and a "detail" page (back bar +
//! track `ColumnView`) swap in place — selecting a node shows the detail, the
//! back button returns to the tree. The panel NEVER touches the DB — it emits
//! intents and the shell pushes query results back.
//!
//! `PopulateDetail` splices the detail store (never rebuilds the `ColumnView` —
//! the RSS-leak invariant); row activation emits `PlayTrack`. The detail column
//! set (art/title-indicator/rating/add + hidden extras) lives in
//! `library_detail_columns`; rating/add emit intents, art is panel-owned (LRU +
//! off-thread fill). The column-config menu, column-width persistence, tree,
//! grouping, and dialogs are shell-mediated.

use std::cell::{Cell, RefCell};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use relm4::{Component, ComponentParts, ComponentSender};

use big_relm4_components::layout::column_chrome::wire_configure_columns_menu;
use big_relm4_components::layout::column_persist::{
    apply_column_widths, wire_column_width_persist,
};

use crate::audio::library_grouping::LibraryGrouping;
use crate::audio::ui::library_detail_columns::{self, ArtCache};
use crate::audio::ui::library_tree::{self, LibraryTreeHandle, LibraryTreeSnapshot};
use crate::audio::ui::{
    library_detail_interactions, library_dialogs, library_grouping_ui, library_io,
    library_tag_editor,
};
use crate::db::models::{Track, WatchFolder};
use crate::i18n::i18n;

/// A bound detail title row's widgets, kept for `SetPlayingPath` re-light:
/// `(file_path, speaker_icon, title_label)`.
pub(super) type DetailIndicator = (String, gtk::Image, gtk::Label);

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
    /// User-facing toast / notification text.
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
    /// Remove a watched folder + its tracks.
    RemoveWatchFolder {
        /// Folder DB id.
        id: i64,
    },
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
    detail_view: gtk::ColumnView,
    /// Right-pane container; swapped between the detail `ColumnView` and an
    /// empty-state placeholder (the view is reattached, never rebuilt).
    content_scroll: gtk::ScrolledWindow,
    /// Currently-playing file path (now-playing indicator target).
    current_path: Rc<RefCell<Option<String>>>,
    /// Bound detail title indicators, pruned on unbind (for re-light).
    detail_indicators: Rc<RefCell<Vec<DetailIndicator>>>,
    /// Set while an inbound `ApplyColumnWidths` runs, so the resize watcher
    /// doesn't echo restored widths back to the shell.
    applying_widths: Rc<Cell<bool>>,
    /// Handle to push tree data after construction (atomic splice, no rebuild).
    library_tree_handle: LibraryTreeHandle,
    /// Grouping dropdown + signal id, synced to the restored preset on boot.
    grouping_controls: library_grouping_ui::GroupingControls,
    /// Weak handle to the currently open tag editor, used for async lookup
    /// results.
    tag_editor: Rc<RefCell<Option<library_tag_editor::TagEditorHandle>>>,
}

impl LibraryPanel {
    /// Wrap tracks as the `BoxedAnyObject`s the store/factories expect.
    fn to_objects(tracks: &[Track]) -> Vec<glib::Object> {
        tracks
            .iter()
            .map(|t| glib::BoxedAnyObject::new(t.clone()).upcast())
            .collect()
    }

    /// Snapshot the current detail tracks (store order).
    fn detail_tracks(&self) -> Vec<Track> {
        (0..self.detail_store.n_items())
            .filter_map(|i| {
                self.detail_store
                    .item(i)
                    .and_downcast::<glib::BoxedAnyObject>()
            })
            .map(|obj| obj.borrow::<Track>().clone())
            .collect()
    }

    /// DB ids of the current detail tracks.
    fn detail_track_ids(&self) -> Vec<i64> {
        (0..self.detail_store.n_items())
            .filter_map(|i| {
                self.detail_store
                    .item(i)
                    .and_downcast::<glib::BoxedAnyObject>()
            })
            .map(|obj| obj.borrow::<Track>().id)
            .collect()
    }

    /// Re-light the detail now-playing indicator on every bound title row by
    /// file-path match.
    fn relight_detail(&self) {
        let current = self.current_path.borrow();
        for (path, icon, label) in self.detail_indicators.borrow().iter() {
            let is_current = current.as_deref() == Some(path.as_str());
            icon.set_visible(is_current);
            if is_current {
                label.add_css_class("now-playing-accent");
            } else {
                label.remove_css_class("now-playing-accent");
            }
        }
    }

    /// Show the detail `ColumnView` when it has rows, otherwise an empty-state
    /// placeholder. The view is reattached, never rebuilt.
    fn refresh_detail_empty_state(&self) {
        if self.detail_store.n_items() == 0 {
            let empty = adw::StatusPage::builder()
                .icon_name("folder-music-symbolic")
                .title(i18n("No Tracks"))
                .description(i18n("Select an item on the left to view its tracks"))
                .vexpand(true)
                .build();
            self.content_scroll.set_child(Some(&empty));
        } else {
            let view: gtk::Widget = self.detail_view.clone().upcast();
            if self.content_scroll.child().as_ref() != Some(&view) {
                self.content_scroll.set_child(Some(&self.detail_view));
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
        let sort_model = gtk::SortListModel::new(Some(detail_store.clone()), None::<gtk::Sorter>);
        let selection = gtk::MultiSelection::new(Some(sort_model.clone()));
        let detail_view = gtk::ColumnView::builder()
            .model(&selection)
            .show_column_separators(false)
            .show_row_separators(false)
            .vexpand(true)
            .build();
        detail_view.add_css_class("library-detail-columnview");

        let current_path: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let detail_indicators: Rc<RefCell<Vec<DetailIndicator>>> =
            Rc::new(RefCell::new(Vec::new()));
        let art_cache: ArtCache = Rc::new(RefCell::new(lru::LruCache::new(
            NonZeroUsize::new(200).unwrap_or(NonZeroUsize::MIN),
        )));
        library_detail_columns::build_detail_columns(
            &detail_view,
            &sender,
            &current_path,
            &detail_indicators,
            &art_cache,
        );
        wire_configure_columns_menu(&detail_view, &i18n("Configure Columns…"), &i18n("Columns"));
        let applying_widths = Rc::new(Cell::new(false));
        {
            let out = sender.clone();
            wire_column_width_persist(&detail_view, &applying_widths, move |spec| {
                let _ = out.output(LibraryPanelOutput::PersistColumnWidths(spec));
            });
        }

        // Row activation → PlayTrack (resolve the visual position to a track).
        {
            let out = sender.clone();
            let sm = sort_model.clone();
            detail_view.connect_activate(move |_, position| {
                if let Some(track) = sm
                    .item(position)
                    .and_downcast::<glib::BoxedAnyObject>()
                    .map(|obj| obj.borrow::<Track>().clone())
                {
                    let _ = out.output(LibraryPanelOutput::PlayTrack(track));
                }
            });
        }
        library_detail_interactions::build_detail_interactions(
            &detail_view,
            &selection,
            &sort_model,
            &sender,
        );

        let content_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .child(&detail_view)
            .build();

        // Single-column layout (mirrors the god-object): the browse tree and the
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
        big_relm4_components::feedback::tooltip::set(&back_button, &i18n("Back to library"));
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
        detail_page.append(&content_scroll);

        root.add_named(&left, Some("browse"));
        root.add_named(&detail_page, Some("detail"));
        root.set_visible_child_name("browse");

        let model = Self {
            detail_store,
            detail_view,
            content_scroll,
            current_path,
            detail_indicators,
            applying_widths,
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
                let objects = Self::to_objects(&tracks);
                // Splice in place — never rebuild the ColumnView (RSS regression).
                self.detail_store
                    .splice(0, self.detail_store.n_items(), &objects);
                self.refresh_detail_empty_state();
                self.relight_detail();
                // Selecting a node swaps the single column to the track list.
                root.set_visible_child_name("detail");
            }
            LibraryPanelInput::SetPlayingPath(path) => {
                *self.current_path.borrow_mut() = path;
                self.relight_detail();
            }
            LibraryPanelInput::ApplyColumnWidths(spec) => {
                self.applying_widths.set(true);
                apply_column_widths(&self.detail_view, &spec);
                self.applying_widths.set(false);
            }
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
                    editor.track_id() == track_id && !editor.apply_musicbrainz_result(metadata)
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
                    library_io::export_detail_m3u8(win, self.detail_tracks(), &sender);
                }
            }
            LibraryPanelInput::ImportPlaylist => {
                if let Some(win) = parent.as_ref() {
                    library_io::import_playlist(win, &sender);
                }
            }
            LibraryPanelInput::SaveDetailAsPlaylist => {
                if let Some(win) = parent.as_ref() {
                    library_io::save_tracks_as_playlist(win, self.detail_track_ids(), &sender);
                }
            }
        }
    }
}
