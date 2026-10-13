// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(not(miri))]
use gtk::glib;

use crate::db::models::Track;
use crate::library::musicbrainz::MbMetadata;

use super::TagEditFields;
#[cfg(not(miri))]
use super::TagEditorHandle;
use super::spec::{
    MusicBrainzFillPlan, TagEditFieldTextRefs, collect_tag_edit_fields, is_editable_text_changed,
    musicbrainz_fill_plan, optional_entry_text, read_only_metadata_rows,
    should_apply_musicbrainz_result, should_offer_musicbrainz_lookup,
};

fn track_with_read_only_metadata() -> Track {
    let mut track =
        Track::new_external_path_with_title("/music/song.flac", Some("Song".to_string()));
    track.duration_secs = 125.0;
    track.bitrate = Some(192_000);
    track.sample_rate = Some(48_000);
    track.channels = Some(2);
    track.format = Some("FLAC".to_string());
    track.file_size = Some(1_536);
    track
}

#[test]
fn optional_entry_text_discards_blank_values() {
    assert_eq!(optional_entry_text(""), None);
    assert_eq!(optional_entry_text("   "), None);
}

#[test]
fn optional_entry_text_trims_real_values() {
    assert_eq!(optional_entry_text("  album  "), Some("album".to_string()));
}

#[test]
fn tag_fields_trim_and_parse_numbers() {
    assert_eq!(
        collect_tag_edit_fields(TagEditFieldTextRefs {
            title: "  Song  ",
            artist: " Artist ",
            album: " Album ",
            album_artist: " Album Artist ",
            genre: " Rock ",
            comment: " Comment ",
            year: " 2026 ",
            track_number: " 7 ",
            disc_number: " 2 ",
        }),
        TagEditFields {
            title: Some("Song".to_string()),
            artist: Some("Artist".to_string()),
            album: Some("Album".to_string()),
            album_artist: Some("Album Artist".to_string()),
            genre: Some("Rock".to_string()),
            comment: Some("Comment".to_string()),
            year: Some("2026".to_string()),
            track_number: Some(7),
            disc_number: Some(2),
        }
    );
}

#[test]
fn tag_fields_drop_blank_and_invalid() {
    assert_eq!(
        collect_tag_edit_fields(TagEditFieldTextRefs {
            title: " ",
            artist: "",
            album: "\n",
            album_artist: "\t",
            genre: "",
            comment: "",
            year: " ",
            track_number: "side a",
            disc_number: "-1",
        }),
        TagEditFields::default()
    );
}

#[test]
fn lookup_needs_artist_and_empty_field() {
    assert!(should_offer_musicbrainz_lookup(true, false, true, true));
    assert!(should_offer_musicbrainz_lookup(true, true, false, true));
    assert!(should_offer_musicbrainz_lookup(true, true, true, false));
    assert!(!should_offer_musicbrainz_lookup(false, false, false, false));
    assert!(!should_offer_musicbrainz_lookup(true, true, true, true));
}

#[test]
fn dirty_detection_compares_exact_text_values() {
    assert!(is_editable_text_changed("Album", "album"));
    assert!(!is_editable_text_changed("Album", "Album"));
}

#[test]
fn read_only_rows_keep_values() {
    let rows = read_only_metadata_rows(&track_with_read_only_metadata());

    assert_eq!(
        rows.iter()
            .map(|row| (row.label, row.value.as_str(), row.is_file_path))
            .collect::<Vec<_>>(),
        vec![
            ("Duration", "2:05", false),
            ("Bitrate", "192 kbps", false),
            ("Sample Rate", "48000 Hz", false),
            ("Channels", "2", false),
            ("Format", "FLAC", false),
            ("File Size", "2 KB", false),
            ("File", "/music/song.flac", true),
        ]
    );
}

#[test]
fn musicbrainz_fill_plan_fills_only_empty_fields() {
    let metadata = MbMetadata {
        genre: Some("Rock".to_string()),
        year: Some(2026),
        track_number: Some(7),
    };

    assert_eq!(
        musicbrainz_fill_plan("", "1999", "", Some(&metadata)),
        MusicBrainzFillPlan {
            genre: Some("Rock".to_string()),
            year: None,
            track_number: Some("7".to_string()),
            lookup_button_label: "✓ Genre, Track #".to_string(),
        }
    );
}

#[test]
fn fill_plan_reports_empty_outcomes() {
    let metadata = MbMetadata {
        genre: Some("Rock".to_string()),
        year: Some(2026),
        track_number: Some(7),
    };

    assert_eq!(
        musicbrainz_fill_plan("", "", "", None),
        MusicBrainzFillPlan {
            genre: None,
            year: None,
            track_number: None,
            lookup_button_label: "Not found".to_string(),
        }
    );
    assert_eq!(
        musicbrainz_fill_plan("Pop", "1999", "2", Some(&metadata)),
        MusicBrainzFillPlan {
            genre: None,
            year: None,
            track_number: None,
            lookup_button_label: "No new data found".to_string(),
        }
    );
}

#[test]
fn musicbrainz_result_requires_all_live_editor_widgets() {
    assert!(should_apply_musicbrainz_result(true, true, true, true));
    assert!(!should_apply_musicbrainz_result(false, true, true, true));
    assert!(!should_apply_musicbrainz_result(true, false, true, true));
    assert!(!should_apply_musicbrainz_result(true, true, false, true));
    assert!(!should_apply_musicbrainz_result(true, true, true, false));
}

#[cfg(not(miri))]
#[test]
fn stale_editor_rejects_result() {
    let handle = TagEditorHandle {
        track_id: 42,
        lookup_button: glib::WeakRef::new(),
        genre_row: glib::WeakRef::new(),
        year_row: glib::WeakRef::new(),
        track_row: glib::WeakRef::new(),
    };

    assert_eq!(handle.track_id(), 42);
    assert!(!handle.apply_musicbrainz_result(None));
}
