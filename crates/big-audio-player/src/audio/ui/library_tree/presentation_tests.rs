// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[cfg(not(miri))]
#[test]
fn row_presentation_makes_dividers_inert_headings() {
    let divider_node = BrowserNode::divider("B");
    let presentation = library_tree_row_presentation(&divider_node);

    assert_eq!(presentation.label, "B");
    assert!(presentation.is_heading);
    assert!(!presentation.is_file_leaf);
    assert!(!presentation.is_activatable);
    assert!(!presentation.is_selectable);
    assert!(!presentation.show_icon);
    assert!(!presentation.show_tracks_button);
    assert_eq!(presentation.track_count_label, None);
    assert_eq!(presentation.avatar, None);
}

#[cfg(not(miri))]
#[test]
fn row_presentation_keeps_file_leaves_for_leaf_binder() {
    let file_node = BrowserNode::track_node("Song", "/music/song.flac", 65.0);
    let presentation = library_tree_row_presentation(&file_node);

    assert_eq!(presentation.label, "Song");
    assert_eq!(presentation.icon_name, "audio-x-generic-symbolic");
    assert!(presentation.is_file_leaf);
    assert!(presentation.is_activatable);
    assert!(presentation.is_selectable);
    assert!(presentation.show_icon);
    assert!(!presentation.show_tracks_button);
}

#[cfg(not(miri))]
#[test]
fn row_presentation_uses_count_badge_and_icon_for_regular_groups() {
    let artist_node = BrowserNode::artist("Alice", 7);
    let presentation = library_tree_row_presentation(&artist_node);

    assert_eq!(presentation.label, "Alice");
    assert_eq!(presentation.icon_name, "avatar-default-symbolic");
    assert_eq!(presentation.track_count_label, Some("7".to_string()));
    assert_eq!(presentation.avatar, None);
    assert!(presentation.show_icon);
    assert!(presentation.show_tracks_button);

    let empty_genre_node = BrowserNode::genre("Ambient", 0);
    let empty_genre_presentation = library_tree_row_presentation(&empty_genre_node);
    assert_eq!(empty_genre_presentation.track_count_label, None);
    assert!(empty_genre_presentation.show_icon);
}

#[cfg(not(miri))]
#[test]
fn row_presentation_uses_avatar_for_album_and_various_artists() {
    let album_node = BrowserNode::album("Debut", "Alice", 3);
    let album_presentation = library_tree_row_presentation(&album_node);

    assert_eq!(
        album_presentation.avatar,
        Some(LibraryTreeAvatarPresentation {
            text: "Debut".to_string(),
            icon_name: None,
        })
    );
    assert!(!album_presentation.show_icon);
    assert_eq!(album_presentation.track_count_label, Some("3".to_string()));

    let various_artists_node = BrowserNode::various_artists(4);
    let various_artists_presentation = library_tree_row_presentation(&various_artists_node);
    assert_eq!(
        various_artists_presentation.avatar,
        Some(LibraryTreeAvatarPresentation {
            text: i18n("Various Artists"),
            icon_name: Some("system-users-symbolic"),
        })
    );
    assert!(!various_artists_presentation.show_icon);
}

#[cfg(not(miri))]
#[test]
fn file_leaf_presentation_handles_badge_fallback_and_detailed_metadata() {
    let fallback_node = BrowserNode::file_node("Loose", "/music/loose.flac");
    fallback_node.set_secondary("3:04");
    assert_eq!(
        file_leaf_presentation(&fallback_node, None),
        FileLeafPresentation {
            duration_label: None,
            artist_label: None,
            fallback_badge_label: Some("3:04".to_string()),
            avatar_text: None,
            has_embedded_art: false,
        }
    );

    let empty_secondary_node = BrowserNode::file_node("Plain", "/music/plain.flac");
    assert_eq!(
        file_leaf_presentation(&empty_secondary_node, None),
        FileLeafPresentation {
            duration_label: None,
            artist_label: None,
            fallback_badge_label: None,
            avatar_text: None,
            has_embedded_art: false,
        }
    );

    let detailed_node = BrowserNode::file_node("Detailed", "/music/detailed.flac");
    detailed_node.set_secondary("Album\t4:20\tAlice\tcover-hash");
    let mut track_with_art = Track::new_external_path("/music/detailed.flac");
    track_with_art.art_hash = Some("cover-hash".to_string());
    assert_eq!(
        file_leaf_presentation(&detailed_node, Some(&track_with_art)),
        FileLeafPresentation {
            duration_label: Some("4:20".to_string()),
            artist_label: Some("Alice".to_string()),
            fallback_badge_label: None,
            avatar_text: Some("Alice".to_string()),
            has_embedded_art: true,
        }
    );

    let minimal_detailed_node = BrowserNode::file_node("Short", "/music/short.flac");
    minimal_detailed_node.set_secondary("Album\t0:59");
    assert_eq!(
        file_leaf_presentation(&minimal_detailed_node, None),
        FileLeafPresentation {
            duration_label: Some("0:59".to_string()),
            artist_label: None,
            fallback_badge_label: None,
            avatar_text: Some("Short".to_string()),
            has_embedded_art: false,
        }
    );

    let no_artist_node = BrowserNode::file_node("Instrumental", "/music/instrumental.flac");
    no_artist_node.set_secondary("Album\t2:10\t\t");
    assert_eq!(
        file_leaf_presentation(&no_artist_node, None),
        FileLeafPresentation {
            duration_label: Some("2:10".to_string()),
            artist_label: None,
            fallback_badge_label: None,
            avatar_text: Some("Instrumental".to_string()),
            has_embedded_art: false,
        }
    );
}
