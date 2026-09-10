//! Locale-aware UI label arrays shared by converter front-ends.
//!
//! Each function returns translated strings (via [`crate::i18n::i18n`]) for a
//! single ComboRow/DropDown picker used in the audio + video converters. IDs
//! stay in the consumer crates; only the user-visible labels live here so the
//! `.pot` extraction sees every string in one place.

use crate::i18n::i18n;

/// Video codec dropdown labels.
#[must_use]
pub fn codec_labels() -> Vec<String> {
    vec![
        i18n("Copy (no re-encode)"),
        i18n("H.264 / AVC"),
        i18n("H.265 / HEVC"),
        i18n("AV1"),
        i18n("VP9"),
        i18n("ProRes"),
    ]
}

/// Video quality dropdown labels.
#[must_use]
pub fn quality_labels() -> Vec<String> {
    vec![
        i18n("Very High"),
        i18n("High"),
        i18n("Good"),
        i18n("Medium"),
        i18n("Low"),
        i18n("Very Low"),
        i18n("Super Low"),
    ]
}

/// Encoder preset dropdown labels.
#[must_use]
pub fn preset_labels() -> Vec<String> {
    vec![
        i18n("Ultra Fast"),
        i18n("Very Fast"),
        i18n("Fast"),
        i18n("Balanced"),
        i18n("Slow"),
        i18n("Very Slow"),
    ]
}

/// Video resolution dropdown labels.
#[must_use]
pub fn resolution_labels() -> Vec<String> {
    vec![
        i18n("Original"),
        i18n("4K (3840×2160)"),
        i18n("1440p (2560×1440)"),
        i18n("1080p (1920×1080)"),
        i18n("720p (1280×720)"),
        i18n("480p (854×480)"),
        i18n("360p (640×360)"),
        i18n("Custom"),
    ]
}

/// Container format dropdown labels.
#[must_use]
pub fn container_labels() -> Vec<String> {
    vec![i18n("MP4"), i18n("MKV"), i18n("MOV"), i18n("WebM")]
}

/// Audio handling mode dropdown labels.
#[must_use]
pub fn audio_handling_labels() -> Vec<String> {
    vec![
        i18n("Copy (no change)"),
        i18n("Re-encode"),
        i18n("Remove audio"),
    ]
}

/// Audio codec dropdown labels.
#[must_use]
pub fn audio_codec_labels() -> Vec<String> {
    vec![
        i18n("AAC"),
        i18n("Opus"),
        i18n("MP3"),
        i18n("AC3 / Dolby"),
        i18n("FLAC (lossless)"),
        i18n("PCM (uncompressed)"),
    ]
}

/// Audio bitrate dropdown labels (video-mode).
#[must_use]
pub fn audio_bitrate_labels() -> Vec<String> {
    vec![
        i18n("64 kbps"),
        i18n("96 kbps"),
        i18n("128 kbps"),
        i18n("192 kbps"),
        i18n("256 kbps"),
        i18n("320 kbps"),
        i18n("Custom"),
    ]
}

/// Audio channels dropdown labels (video-mode).
#[must_use]
pub fn audio_channels_labels() -> Vec<String> {
    vec![
        i18n("Auto"),
        i18n("Mono"),
        i18n("Stereo"),
        i18n("5.1 Surround"),
        i18n("Custom"),
    ]
}

/// Subtitle handling dropdown labels.
#[must_use]
pub fn subtitle_labels() -> Vec<String> {
    vec![
        i18n("Extract to File"),
        i18n("Keep Embedded"),
        i18n("Burn into Video"),
        i18n("None"),
    ]
}

/// Audio output format dropdown labels (audio-mode).
#[must_use]
pub fn audio_format_labels() -> Vec<String> {
    vec![
        i18n("Copy (no re-encode)"),
        i18n("MP3"),
        i18n("OGG Vorbis"),
        i18n("FLAC (lossless)"),
        i18n("WAV (uncompressed)"),
        i18n("AAC"),
        i18n("Opus"),
    ]
}

/// Audio bitrate dropdown labels (audio-mode).
#[must_use]
pub fn audio_bitrate_labels_audio_mode() -> Vec<String> {
    vec![
        i18n("32 kbps"),
        i18n("64 kbps"),
        i18n("128 kbps"),
        i18n("192 kbps"),
        i18n("256 kbps"),
        i18n("320 kbps"),
    ]
}

/// Audio channels dropdown labels (audio-mode).
#[must_use]
pub fn audio_channels_labels_audio_mode() -> Vec<String> {
    vec![i18n("Original"), i18n("Mono"), i18n("Stereo")]
}

/// Video editor speed-control labels.
#[must_use]
pub fn speed_labels() -> Vec<String> {
    // Numeric speeds: not translated.
    vec![
        "0.25×".to_string(),
        "0.5×".to_string(),
        "0.75×".to_string(),
        "1×".to_string(),
        "1.25×".to_string(),
        "1.5×".to_string(),
        "2×".to_string(),
    ]
}
