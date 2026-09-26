// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure copy/action specs for the playlist library dialog.

use crate::i18n::i18n;

use super::PlaylistLibraryDialogOutput;
use big_media_runtime::i18n::gettext_noop;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PlaylistLibraryWindowSpec {
    pub(super) title: &'static str,
    pub(super) content_title: &'static str,
    pub(super) default_width: i32,
    pub(super) default_height: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SavedPlaylistListState {
    pub(super) content_title: String,
    pub(super) is_empty: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EmptyPlaylistStateSpec {
    pub(super) icon_name: &'static str,
    pub(super) title: &'static str,
    pub(super) description: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PlaylistActionSpec {
    pub(super) icon_name: &'static str,
    pub(super) label: &'static str,
    pub(super) action_kind: PlaylistActionKind,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PlaylistActionKind {
    Import,
    ImportFolder,
    ExportAll,
    GenerateLoad(&'static str),
}

impl PlaylistActionKind {
    pub(super) fn to_output(&self) -> PlaylistLibraryDialogOutput {
        match self {
            Self::Import => PlaylistLibraryDialogOutput::Import,
            Self::ImportFolder => PlaylistLibraryDialogOutput::ImportFolder,
            Self::ExportAll => PlaylistLibraryDialogOutput::ExportAll,
            Self::GenerateLoad(kind) => PlaylistLibraryDialogOutput::GenerateLoad(kind.to_string()),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SavedPlaylistRowSpec {
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) load_label: &'static str,
    pub(super) rename_tooltip: String,
    pub(super) export_tooltip: String,
    pub(super) delete_tooltip: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct NamePromptSpec {
    pub(super) heading: &'static str,
    pub(super) entry_label: &'static str,
    pub(super) placeholder: &'static str,
    pub(super) prefill: String,
    pub(super) accept_label: &'static str,
    pub(super) cancel_label: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct DeletePromptSpec {
    pub(super) heading: &'static str,
    pub(super) playlist_name: String,
    pub(super) accept_label: &'static str,
    pub(super) cancel_label: &'static str,
    pub(super) accept_css_class: &'static str,
}

/// Smart-generator presets, keyed to the shell's `generate_smart_to_queue` kinds.
const GENERATORS: [(&str, &str, &str); 3] = [
    ("most-played", "Most Played", "starred-symbolic"),
    (
        "recently-added",
        "Recently Added",
        "document-open-recent-symbolic",
    ),
    ("never-played", "Never Played", "audio-x-generic-symbolic"),
];

pub(super) fn playlist_library_window_spec() -> PlaylistLibraryWindowSpec {
    PlaylistLibraryWindowSpec {
        title: gettext_noop("Manage Playlists"),
        content_title: gettext_noop("Your Playlists"),
        default_width: 820,
        default_height: 640,
    }
}

pub(super) fn saved_playlist_list_state(count: usize) -> SavedPlaylistListState {
    if count == 0 {
        SavedPlaylistListState {
            content_title: gettext_noop("Your Playlists").to_string(),
            is_empty: true,
        }
    } else {
        SavedPlaylistListState {
            content_title: format!("Your Playlists ({count})"),
            is_empty: false,
        }
    }
}

pub(super) fn empty_playlist_state_spec() -> EmptyPlaylistStateSpec {
    EmptyPlaylistStateSpec {
        icon_name: "view-list-symbolic",
        title: gettext_noop("No Saved Playlists"),
        description: gettext_noop(
            "Save the current queue, generate one from your stats, or import a file",
        ),
    }
}

pub(super) fn import_export_action_specs() -> Vec<PlaylistActionSpec> {
    vec![
        PlaylistActionSpec {
            icon_name: "document-import-symbolic",
            label: gettext_noop("Import File…"),
            action_kind: PlaylistActionKind::Import,
        },
        PlaylistActionSpec {
            icon_name: "folder-open-symbolic",
            label: gettext_noop("Import Folder…"),
            action_kind: PlaylistActionKind::ImportFolder,
        },
        PlaylistActionSpec {
            icon_name: "document-save-symbolic",
            label: gettext_noop("Export All…"),
            action_kind: PlaylistActionKind::ExportAll,
        },
    ]
}

pub(super) fn generator_action_specs() -> Vec<PlaylistActionSpec> {
    GENERATORS
        .into_iter()
        .map(|(kind, label, icon_name)| PlaylistActionSpec {
            icon_name,
            label,
            action_kind: PlaylistActionKind::GenerateLoad(kind),
        })
        .collect()
}

pub(super) fn saved_playlist_row_spec(name: &str, count: i64, secs: f64) -> SavedPlaylistRowSpec {
    SavedPlaylistRowSpec {
        title: name.to_string(),
        subtitle: saved_playlist_subtitle(count, secs),
        load_label: gettext_noop("Load"),
        rename_tooltip: format!("Rename playlist: {name}"),
        export_tooltip: format!("Export playlist: {name}"),
        delete_tooltip: format!("Delete playlist: {name}"),
    }
}

fn saved_playlist_subtitle(count: i64, secs: f64) -> String {
    let track_label = if count == 1 { "track" } else { "tracks" };
    format!("{count} {track_label} · {} min", (secs as i64) / 60)
}

pub(super) fn localized_saved_playlist_subtitle(count: i64, secs: f64) -> String {
    let track_label = if count == 1 {
        i18n("track")
    } else {
        i18n("tracks")
    };
    format!(
        "{count} {track_label} · {} {}",
        (secs as i64) / 60,
        i18n("min")
    )
}

pub(super) fn save_current_queue_prompt_spec() -> NamePromptSpec {
    NamePromptSpec {
        heading: gettext_noop("Save Current Queue"),
        entry_label: "Playlist name",
        placeholder: gettext_noop("Name for the new playlist"),
        prefill: String::new(),
        accept_label: "Save",
        cancel_label: gettext_noop("Cancel"),
    }
}

pub(super) fn rename_playlist_prompt_spec(current_name: &str) -> NamePromptSpec {
    NamePromptSpec {
        heading: gettext_noop("Rename Playlist"),
        entry_label: "Playlist name",
        placeholder: gettext_noop("New name"),
        prefill: current_name.to_string(),
        accept_label: "Save",
        cancel_label: gettext_noop("Cancel"),
    }
}

pub(super) fn delete_playlist_prompt_spec(name: &str) -> DeletePromptSpec {
    DeletePromptSpec {
        heading: gettext_noop("Delete Playlist?"),
        playlist_name: name.to_string(),
        accept_label: "Delete",
        cancel_label: gettext_noop("Cancel"),
        accept_css_class: "destructive-action",
    }
}

pub(super) fn localized_delete_playlist_description(name: &str) -> String {
    format!(
        "{} \u{201C}{name}\u{201D}?",
        i18n("This will permanently delete")
    )
}

pub(super) fn is_name_prompt_accept_enabled(text: &str) -> bool {
    !text.trim().is_empty()
}

pub(super) fn accepted_playlist_name(text: &str) -> Option<String> {
    let name = text.trim();
    (!name.is_empty()).then(|| name.to_string())
}

pub(super) fn name_prompt_focus_selection() -> (i32, i32) {
    (0, -1)
}
