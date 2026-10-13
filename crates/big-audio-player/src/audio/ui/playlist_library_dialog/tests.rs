// SPDX-License-Identifier: GPL-3.0-or-later

use super::spec::{
    EmptyPlaylistStateSpec, NamePromptSpec, PlaylistActionKind, PlaylistActionSpec,
    PlaylistLibraryWindowSpec, SavedPlaylistListState, SavedPlaylistRowSpec,
    accepted_playlist_name, delete_playlist_prompt_spec, empty_playlist_state_spec,
    generator_action_specs, import_export_action_specs, is_name_prompt_accept_enabled,
    localized_delete_playlist_description, localized_saved_playlist_subtitle,
    name_prompt_focus_selection, playlist_library_window_spec, rename_playlist_prompt_spec,
    save_current_queue_prompt_spec, saved_playlist_list_state, saved_playlist_row_spec,
};

#[test]
fn library_window_is_resizable() {
    assert_eq!(
        playlist_library_window_spec(),
        PlaylistLibraryWindowSpec {
            title: "Manage Playlists",
            content_title: "Your Playlists",
            default_width: 820,
            default_height: 640,
        }
    );
}

#[test]
fn saved_list_state_empty_and_counted() {
    assert_eq!(
        saved_playlist_list_state(0),
        SavedPlaylistListState {
            content_title: "Your Playlists".to_string(),
            is_empty: true,
        }
    );
    assert_eq!(
        saved_playlist_list_state(3),
        SavedPlaylistListState {
            content_title: "Your Playlists (3)".to_string(),
            is_empty: false,
        }
    );
    assert_eq!(
        empty_playlist_state_spec(),
        EmptyPlaylistStateSpec {
            icon_name: "view-list-symbolic",
            title: "No Saved Playlists",
            description: "Save the current queue, generate one from your stats, or import a file",
        }
    );
}

#[test]
fn playlist_library_action_specs_preserve_sidebar_order() {
    assert_eq!(
        import_export_action_specs(),
        vec![
            PlaylistActionSpec {
                icon_name: "document-import-symbolic",
                label: "Import File…",
                action_kind: PlaylistActionKind::Import,
            },
            PlaylistActionSpec {
                icon_name: "folder-open-symbolic",
                label: "Import Folder…",
                action_kind: PlaylistActionKind::ImportFolder,
            },
            PlaylistActionSpec {
                icon_name: "document-save-symbolic",
                label: "Export All…",
                action_kind: PlaylistActionKind::ExportAll,
            },
        ]
    );
    assert_eq!(
        generator_action_specs(),
        vec![
            PlaylistActionSpec {
                icon_name: "starred-symbolic",
                label: "Most Played",
                action_kind: PlaylistActionKind::GenerateLoad("most-played"),
            },
            PlaylistActionSpec {
                icon_name: "document-open-recent-symbolic",
                label: "Recently Added",
                action_kind: PlaylistActionKind::GenerateLoad("recently-added"),
            },
            PlaylistActionSpec {
                icon_name: "audio-x-generic-symbolic",
                label: "Never Played",
                action_kind: PlaylistActionKind::GenerateLoad("never-played"),
            },
        ]
    );
}

#[test]
fn saved_row_formats_counts_and_minutes() {
    assert_eq!(
        saved_playlist_row_spec("Road Mix"),
        SavedPlaylistRowSpec {
            title: "Road Mix".to_string(),
            load_label: "Load",
            rename_tooltip: "Rename playlist: Road Mix".to_string(),
            export_tooltip: "Export playlist: Road Mix".to_string(),
            delete_tooltip: "Delete playlist: Road Mix".to_string(),
        }
    );
    assert_eq!(
        localized_saved_playlist_subtitle(1, 125.0),
        "1 track · 2 min"
    );
    assert_eq!(
        localized_saved_playlist_subtitle(4, 360.0),
        "4 tracks · 6 min"
    );
}

#[test]
fn name_prompt_trims_blank_values() {
    assert_eq!(
        save_current_queue_prompt_spec(),
        NamePromptSpec {
            heading: "Save Current Queue",
            entry_label: "Playlist name",
            placeholder: "Name for the new playlist",
            prefill: String::new(),
            accept_label: "Save",
            cancel_label: "Cancel",
        }
    );
    assert_eq!(
        rename_playlist_prompt_spec("Road Mix"),
        NamePromptSpec {
            heading: "Rename Playlist",
            entry_label: "Playlist name",
            placeholder: "New name",
            prefill: "Road Mix".to_string(),
            accept_label: "Save",
            cancel_label: "Cancel",
        }
    );
    assert!(!is_name_prompt_accept_enabled("   "));
    assert!(is_name_prompt_accept_enabled(" Road Mix "));
    assert_eq!(
        accepted_playlist_name(" Road Mix "),
        Some("Road Mix".to_string())
    );
    assert_eq!(accepted_playlist_name(" \t "), None);
}

#[test]
fn name_prompt_focus_selects_entire_prefill() {
    assert_eq!(name_prompt_focus_selection(), (0, -1));
}

#[test]
fn delete_playlist_prompt_spec_keeps_destructive_copy() {
    let prompt_spec = delete_playlist_prompt_spec("Road Mix");
    assert_eq!(prompt_spec.heading, "Delete Playlist?");
    assert_eq!(prompt_spec.playlist_name, "Road Mix");
    assert_eq!(prompt_spec.accept_label, "Delete");
    assert_eq!(prompt_spec.cancel_label, "Cancel");
    assert_eq!(prompt_spec.accept_css_class, "destructive-action");
    assert_eq!(
        localized_delete_playlist_description("Road Mix"),
        "This will permanently delete “Road Mix”?"
    );
}

#[test]
#[ignore = "requires an isolated GTK session"]
fn hidden_playlist_window_finalizes_with_its_controller() {
    use gtk::prelude::*;
    use relm4::{Component, ComponentController};
    use std::{cell::Cell, rc::Rc};
    gtk::init().unwrap();
    adw::init().unwrap();
    let context = gtk::glib::MainContext::default();
    let finalized = Rc::new(Cell::new(0));
    struct Finalized(Rc<Cell<usize>>);
    impl Drop for Finalized {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for cycle in 1..=25 {
        let controller = super::PlaylistLibraryDialog::builder().launch(()).detach();
        // SAFETY: this test owns the key and never reads its value.
        unsafe {
            controller.widget().set_qdata(
                gtk::glib::Quark::from_str("playlist-window-finalized"),
                Finalized(finalized.clone()),
            );
        }
        drop(controller);
        while context.pending() {
            context.iteration(false);
        }
        assert_eq!(
            finalized.get(),
            cycle,
            "never-presented window survived controller teardown"
        );
    }
}
