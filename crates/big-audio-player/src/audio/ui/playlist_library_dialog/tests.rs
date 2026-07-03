// SPDX-License-Identifier: GPL-3.0-or-later

use super::spec::{
    accepted_playlist_name, delete_playlist_description, delete_playlist_prompt_spec,
    empty_playlist_state_spec, generator_action_specs, import_export_action_specs,
    is_name_prompt_accept_enabled, localized_delete_playlist_description,
    localized_saved_playlist_subtitle, name_prompt_focus_selection, playlist_library_window_spec,
    rename_playlist_prompt_spec, save_current_queue_prompt_spec, saved_playlist_list_state,
    saved_playlist_row_spec, EmptyPlaylistStateSpec, NamePromptSpec, PlaylistActionKind,
    PlaylistActionSpec, PlaylistLibraryWindowSpec, SavedPlaylistListState, SavedPlaylistRowSpec,
};

#[test]
fn playlist_library_window_spec_keeps_resizable_dialog_chrome() {
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
fn saved_playlist_list_state_names_empty_and_counted_content() {
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
fn saved_playlist_row_spec_formats_counts_minutes_and_tooltips() {
    assert_eq!(
        saved_playlist_row_spec("Road Mix", 1, 125.0),
        SavedPlaylistRowSpec {
            title: "Road Mix".to_string(),
            subtitle: "1 track · 2 min".to_string(),
            load_label: "Load",
            rename_tooltip: "Rename playlist: Road Mix".to_string(),
            export_tooltip: "Export playlist: Road Mix".to_string(),
            delete_tooltip: "Delete playlist: Road Mix".to_string(),
        }
    );
    assert_eq!(
        saved_playlist_row_spec("Night Mix", 4, 360.0).subtitle,
        "4 tracks · 6 min"
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
fn name_prompt_spec_and_acceptance_trim_blank_values() {
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
        delete_playlist_description("Road Mix"),
        "This will permanently delete “Road Mix”?"
    );
    assert_eq!(
        localized_delete_playlist_description("Road Mix"),
        "This will permanently delete “Road Mix”?"
    );
}
