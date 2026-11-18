//! App-local Relm4 library panel (push-model).
//!
//! A grouping/search bar over the library tree. The panel never touches the
//! DB; it emits intents and the shell pushes query results back. Tree,
//! grouping and dialogs are shell-mediated.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::audio::library_grouping::LibraryGrouping;
use crate::audio::ui::library_tree::{self, LibraryTreeHandle, LibraryTreeSnapshot};
use crate::audio::ui::{library_dialogs, library_grouping_ui, library_io, library_tag_editor};
use crate::db::models::{Track, WatchFolder};

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
    /// Import a playlist file into the library (panel picks the file).
    ImportPlaylist,
}

/// Intents the library panel emits for the shell to act on.
#[derive(Debug)]
pub enum LibraryPanelOutput {
    /// Play a track from the tree's menu (shell adds it to the queue + plays).
    PlayTrack(Track),
    /// Append a track to the queue (Add to Queue).
    EnqueueTrack(Track),
    /// Show the "add to playlist" dialog for a track (shell owns the dialog).
    ShowAddToPlaylist(Track),
    /// Reveal a file in the desktop file manager.
    RevealInFileManager(String),
    /// A library tree node was activated; the shell queues its tracks.
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
    /// Grouping preset changed; shell re-runs the tree query.
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
    /// Import an m3u/m3u8/pls file (shell parses + inserts).
    ImportPlaylistFile {
        /// Picked file path.
        path: PathBuf,
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

/// Relm4 library panel component.
pub struct LibraryPanel {
    /// Handle to push tree data after construction (atomic splice, no rebuild).
    library_tree_handle: LibraryTreeHandle,
    /// Grouping dropdown + signal id, synced to the restored preset on boot.
    grouping_controls: library_grouping_ui::GroupingControls,
    /// Weak handle to the currently open tag editor, used for async lookup
    /// results.
    tag_editor: Rc<RefCell<Option<library_tag_editor::TagEditorHandle>>>,
}

impl Component for LibraryPanel {
    type Init = ();
    type Input = LibraryPanelInput;
    type Output = LibraryPanelOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let (grouping_bar, grouping_controls) = library_grouping_ui::build_grouping_bar(&sender);
        root.append(&grouping_bar);
        let (tree_view, library_tree_handle) = library_tree::build_library_tree(&sender);
        let tree_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&tree_view)
            .build();
        root.append(&tree_scroll);

        let model = Self {
            library_tree_handle,
            grouping_controls,
            tag_editor: Rc::new(RefCell::new(None)),
        };
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        let parent = root.root().and_downcast::<gtk::Window>();
        match message {
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
            LibraryPanelInput::ImportPlaylist => {
                if let Some(win) = parent.as_ref() {
                    library_io::import_playlist(win, &sender);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_editor_handle_clears_on_match() {
        assert!(should_clear_tag_editor_handle(42, 42, false));
        assert!(!should_clear_tag_editor_handle(42, 7, false));
        assert!(!should_clear_tag_editor_handle(42, 42, true));
    }
}
