// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure tag editor specs for metadata rows, field collection, and MusicBrainz fill rules.

use crate::db::models::Track;
use crate::i18n::i18n;
use crate::library::musicbrainz::MbMetadata;

use super::TagEditFields;
use big_media_runtime::i18n::gettext_noop;

#[derive(Debug, Clone, Copy)]
pub(super) struct TagEditFieldTextRefs<'a> {
    pub(super) title: &'a str,
    pub(super) artist: &'a str,
    pub(super) album: &'a str,
    pub(super) album_artist: &'a str,
    pub(super) genre: &'a str,
    pub(super) comment: &'a str,
    pub(super) year: &'a str,
    pub(super) track_number: &'a str,
    pub(super) disc_number: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReadOnlyMetadataRow {
    pub(super) label: &'static str,
    pub(super) value: String,
    pub(super) is_file_path: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MusicBrainzFillPlan {
    pub(super) genre: Option<String>,
    pub(super) year: Option<String>,
    pub(super) track_number: Option<String>,
    pub(super) lookup_button_label: String,
}

/// Trim and collapse blank entry text to `None` (a cleared tag).
pub(super) fn optional_entry_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn optional_u32_entry(text: &str) -> Option<u32> {
    text.trim().parse().ok()
}

pub(super) fn collect_tag_edit_fields(fields: TagEditFieldTextRefs<'_>) -> TagEditFields {
    TagEditFields {
        title: optional_entry_text(fields.title),
        artist: optional_entry_text(fields.artist),
        album: optional_entry_text(fields.album),
        album_artist: optional_entry_text(fields.album_artist),
        genre: optional_entry_text(fields.genre),
        comment: optional_entry_text(fields.comment),
        year: optional_entry_text(fields.year),
        track_number: optional_u32_entry(fields.track_number),
        disc_number: optional_u32_entry(fields.disc_number),
    }
}

pub(super) fn should_offer_musicbrainz_lookup(
    has_artist: bool,
    has_genre: bool,
    has_year: bool,
    has_track_number: bool,
) -> bool {
    has_artist && (!has_genre || !has_year || !has_track_number)
}

pub(super) fn is_editable_text_changed(current_text: &str, original_text: &str) -> bool {
    current_text != original_text
}

pub(super) fn read_only_metadata_rows(track: &Track) -> Vec<ReadOnlyMetadataRow> {
    vec![
        ReadOnlyMetadataRow {
            label: gettext_noop("Duration"),
            value: track.duration_display(),
            is_file_path: false,
        },
        ReadOnlyMetadataRow {
            label: gettext_noop("Bitrate"),
            value: track.bitrate.map_or_else(
                || "-".into(),
                |bitrate| {
                    big_media_components::audio::labels::bitrate_kbps_label_from_bps(bitrate.into())
                },
            ),
            is_file_path: false,
        },
        ReadOnlyMetadataRow {
            label: gettext_noop("Sample Rate"),
            value: track
                .sample_rate
                .map_or_else(|| "-".into(), |rate| format!("{rate} Hz")),
            is_file_path: false,
        },
        ReadOnlyMetadataRow {
            label: gettext_noop("Channels"),
            value: track
                .channels
                .map_or_else(|| "-".into(), |channels| channels.to_string()),
            is_file_path: false,
        },
        ReadOnlyMetadataRow {
            label: gettext_noop("Format"),
            value: track.format.as_deref().unwrap_or("-").to_string(),
            is_file_path: false,
        },
        ReadOnlyMetadataRow {
            label: gettext_noop("File Size"),
            value: track.file_size.map_or_else(
                || "-".into(),
                |size| big_media_components::bytes::decimal_compact(size.max(0) as u64),
            ),
            is_file_path: false,
        },
        ReadOnlyMetadataRow {
            label: gettext_noop("File"),
            value: track.file_path.clone(),
            is_file_path: true,
        },
    ]
}

pub(super) fn musicbrainz_fill_plan(
    current_genre: &str,
    current_year: &str,
    current_track_number: &str,
    meta: Option<&MbMetadata>,
) -> MusicBrainzFillPlan {
    let Some(meta) = meta else {
        return MusicBrainzFillPlan {
            genre: None,
            year: None,
            track_number: None,
            lookup_button_label: i18n("Not found"),
        };
    };

    let genre = current_genre
        .is_empty()
        .then(|| meta.genre.clone())
        .flatten();
    let year = current_year
        .is_empty()
        .then(|| meta.year.map(|year| year.to_string()))
        .flatten();
    let track_number = current_track_number
        .is_empty()
        .then(|| {
            meta.track_number
                .map(|track_number| track_number.to_string())
        })
        .flatten();

    let mut filled_field_names: Vec<&str> = Vec::new();
    if genre.is_some() {
        filled_field_names.push("Genre");
    }
    if year.is_some() {
        filled_field_names.push("Year");
    }
    if track_number.is_some() {
        filled_field_names.push("Track #");
    }

    let lookup_button_label = if filled_field_names.is_empty() {
        i18n("No new data found")
    } else {
        format!("✓ {}", filled_field_names.join(", "))
    };

    MusicBrainzFillPlan {
        genre,
        year,
        track_number,
        lookup_button_label,
    }
}

pub(super) fn should_apply_musicbrainz_result(
    has_lookup_button: bool,
    has_genre_row: bool,
    has_year_row: bool,
    has_track_row: bool,
) -> bool {
    has_lookup_button && has_genre_row && has_year_row && has_track_row
}
