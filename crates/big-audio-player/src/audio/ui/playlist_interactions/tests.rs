// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    delete_storage_body, delete_storage_heading, download_episode_label,
    downloadable_episode_items, is_queue_delete_key, remove_from_queue_label,
    should_show_single_track_actions, valid_reorder_source_index,
};
use crate::db::models::Track;
use gtk::gdk;

fn track(path: &str, title: &str) -> Track {
    Track::new_external_path_with_title(path, Some(title.to_string()))
}

#[test]
fn delete_storage_heading_marks_plural_count() {
    assert_eq!(delete_storage_heading(1), "Delete from Storage");
    assert_eq!(delete_storage_heading(3), "Delete from Storage (3)");
}

#[test]
fn delete_storage_body_names_single_track_or_count() {
    assert_eq!(
        delete_storage_body(1, Some("First Song")),
        "Move this file to the trash?\n\nFirst Song"
    );
    assert_eq!(
        delete_storage_body(3, None),
        "Move these files to the trash?\n\n3 tracks"
    );
}

#[test]
fn queue_delete_key_accepts_delete_and_backspace_only() {
    assert!(is_queue_delete_key(gdk::Key::Delete));
    assert!(is_queue_delete_key(gdk::Key::BackSpace));
    assert!(!is_queue_delete_key(gdk::Key::space));
}

#[test]
fn reorder_source_index_rejects_negative_payloads() {
    assert_eq!(valid_reorder_source_index(-1), None);
    assert_eq!(valid_reorder_source_index(0), Some(0));
    assert_eq!(valid_reorder_source_index(7), Some(7));
}

#[test]
fn queue_context_menu_labels_follow_selection_count() {
    assert_eq!(remove_from_queue_label(1), "Remove from Queue");
    assert_eq!(remove_from_queue_label(3), "Remove from Queue (3)");
    assert!(should_show_single_track_actions(1));
    assert!(!should_show_single_track_actions(2));
}

#[test]
fn downloadable_episode_items_keep_only_http_tracks() {
    let selected = vec![
        (0, track("https://example.test/podcast.mp3", "Episode")),
        (1, track("http://example.test/bonus.mp3", "Bonus")),
        (2, track("/music/song.flac", "Song")),
    ];

    assert_eq!(
        downloadable_episode_items(&selected),
        vec![
            (
                "https://example.test/podcast.mp3".to_string(),
                "Episode".to_string(),
            ),
            (
                "http://example.test/bonus.mp3".to_string(),
                "Bonus".to_string(),
            ),
        ]
    );
    assert_eq!(download_episode_label(0), None);
    assert_eq!(
        download_episode_label(1),
        Some("Download Episode…".to_string())
    );
    assert_eq!(
        download_episode_label(2),
        Some("Download Episodes… (2)".to_string())
    );
}
