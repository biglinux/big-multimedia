//! Pure row presentation decisions for the audio library tree.

use crate::audio::browser_node::{BrowserNode, BrowserNodeKind};
use crate::db::models::Track;
use crate::i18n::i18n;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LibraryTreeAvatarPresentation {
    pub(super) text: String,
    pub(super) icon_name: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LibraryTreeRowPresentation {
    pub(super) label: String,
    pub(super) icon_name: String,
    pub(super) track_count_label: Option<String>,
    pub(super) avatar: Option<LibraryTreeAvatarPresentation>,
    pub(super) is_heading: bool,
    pub(super) is_file_leaf: bool,
    pub(super) is_activatable: bool,
    pub(super) is_selectable: bool,
    pub(super) show_icon: bool,
    pub(super) show_tracks_button: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FileLeafPresentation {
    pub(super) duration_label: Option<String>,
    pub(super) artist_label: Option<String>,
    pub(super) fallback_badge_label: Option<String>,
    pub(super) avatar_text: Option<String>,
    pub(super) has_embedded_art: bool,
}

pub(super) fn library_tree_row_presentation(node: &BrowserNode) -> LibraryTreeRowPresentation {
    let kind = node.kind();
    if kind == BrowserNodeKind::Divider {
        return LibraryTreeRowPresentation {
            label: node.label(),
            icon_name: String::new(),
            track_count_label: None,
            avatar: None,
            is_heading: true,
            is_file_leaf: false,
            is_activatable: false,
            is_selectable: false,
            show_icon: false,
            show_tracks_button: false,
        };
    }

    if kind == BrowserNodeKind::File {
        return LibraryTreeRowPresentation {
            label: node.label(),
            icon_name: node.icon_name(),
            track_count_label: None,
            avatar: None,
            is_heading: false,
            is_file_leaf: true,
            is_activatable: true,
            is_selectable: true,
            show_icon: true,
            show_tracks_button: false,
        };
    }

    let is_album = kind == BrowserNodeKind::Album;
    let is_various_artists = kind == BrowserNodeKind::VariousArtists;
    let avatar = if is_album {
        Some(LibraryTreeAvatarPresentation {
            text: node.label(),
            icon_name: None,
        })
    } else if is_various_artists {
        Some(LibraryTreeAvatarPresentation {
            text: i18n("Various Artists"),
            icon_name: Some("system-users-symbolic"),
        })
    } else {
        None
    };
    let track_count = node.track_count();

    LibraryTreeRowPresentation {
        label: node.label(),
        icon_name: node.icon_name(),
        track_count_label: (track_count > 0).then(|| track_count.to_string()),
        avatar,
        is_heading: false,
        is_file_leaf: false,
        is_activatable: true,
        is_selectable: true,
        show_icon: !is_album && !is_various_artists,
        show_tracks_button: true,
    }
}

pub(super) fn file_leaf_presentation(
    node: &BrowserNode,
    source_track: Option<&Track>,
) -> FileLeafPresentation {
    let secondary = node.secondary();
    let secondary_fields: Vec<&str> = secondary.splitn(4, '\t').collect();
    if secondary_fields.len() < 2 {
        return FileLeafPresentation {
            duration_label: None,
            artist_label: None,
            fallback_badge_label: (!secondary.is_empty()).then_some(secondary),
            avatar_text: None,
            has_embedded_art: false,
        };
    }

    let duration = secondary_fields[1].to_string();
    let artist = secondary_fields.get(2).copied().unwrap_or("");
    let artist_label = (!artist.is_empty()).then(|| artist.to_string());
    let avatar_text = artist_label.clone().unwrap_or_else(|| node.label());
    let has_embedded_art = source_track
        .and_then(|track| track.art_hash.as_deref())
        .is_some_and(|art_hash| !art_hash.is_empty());

    FileLeafPresentation {
        duration_label: Some(duration),
        artist_label,
        fallback_badge_label: None,
        avatar_text: Some(avatar_text),
        has_embedded_art,
    }
}
