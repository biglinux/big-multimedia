// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure copy/action specs for the playlist library dialog.

use crate::i18n::i18n;

use super::PlaylistLibraryDialogOutput;
use big_media_runtime::i18n::gettext_noop;
use big_relm4_components::layout::name_prompt::BigNamePromptSpec;

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
    pub(super) load_label: &'static str,
    pub(super) rename_tooltip: String,
    pub(super) export_tooltip: String,
    pub(super) delete_tooltip: String,
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
    (
        "most-played",
        gettext_noop("Most Played"),
        "starred-symbolic",
    ),
    (
        "recently-added",
        gettext_noop("Recently Added"),
        "document-open-recent-symbolic",
    ),
    (
        "never-played",
        gettext_noop("Never Played"),
        "audio-x-generic-symbolic",
    ),
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

pub(super) fn saved_playlist_row_spec(name: &str) -> SavedPlaylistRowSpec {
    SavedPlaylistRowSpec {
        title: name.to_string(),
        load_label: gettext_noop("Load"),
        rename_tooltip: format!("{}: {name}", i18n("Rename playlist")),
        export_tooltip: format!("{}: {name}", i18n("Export playlist")),
        delete_tooltip: format!("{}: {name}", i18n("Delete playlist")),
    }
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

pub(super) fn save_current_queue_prompt_spec() -> BigNamePromptSpec {
    BigNamePromptSpec {
        title: i18n("Save Current Queue"),
        description: Some(i18n("Name for the new playlist")),
        entry_title: i18n("Playlist name"),
        prefill: String::new(),
        accept_label: i18n("Save"),
        content_width: 420,
        content_height: 240,
    }
}

pub(super) fn rename_playlist_prompt_spec(current_name: &str) -> BigNamePromptSpec {
    BigNamePromptSpec {
        title: i18n("Rename Playlist"),
        description: Some(i18n("New name")),
        entry_title: i18n("Playlist name"),
        prefill: current_name.to_string(),
        accept_label: i18n("Save"),
        content_width: 420,
        content_height: 240,
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
