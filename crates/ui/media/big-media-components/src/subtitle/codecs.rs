// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared subtitle codec classification.
//!
//! Text vs bitmap subtitle distinction drives whether we can stream-copy a
//! subtitle track or must re-encode it.

/// Sidecar extensions that always hold text (re-encodable) subtitles.
pub const TEXT_SUBTITLE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "sub", "vtt"];
/// All recognised subtitle sidecar extensions, text or bitmap.
pub const SUBTITLE_FILE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "sub", "vtt", "idx", "sup"];

/// `true` for the bitmap subtitle codecs that cannot be converted to text.
#[must_use]
pub fn is_bitmap_subtitle_codec(codec: &str) -> bool {
    matches!(
        codec,
        "hdmv_pgs_subtitle"
            | "pgssub"
            | "dvd_subtitle"
            | "dvdsub"
            | "dvb_subtitle"
            | "dvbsub"
            | "xsub"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_subtitle_codecs_cover_disc_and_broadcast_formats() {
        for codec in [
            "hdmv_pgs_subtitle",
            "pgssub",
            "dvd_subtitle",
            "dvdsub",
            "dvb_subtitle",
            "dvbsub",
            "xsub",
        ] {
            assert!(is_bitmap_subtitle_codec(codec), "{codec}");
        }
    }

    #[test]
    fn text_subtitle_codecs_are_not_bitmap() {
        for codec in ["subrip", "ass", "webvtt", "mov_text", ""] {
            assert!(!is_bitmap_subtitle_codec(codec), "{codec}");
        }
    }

    #[test]
    fn subtitle_extension_sets_separate_text_from_bitmap_sidecars() {
        assert!(TEXT_SUBTITLE_EXTENSIONS.contains(&"srt"));
        assert!(!TEXT_SUBTITLE_EXTENSIONS.contains(&"sup"));
        assert!(SUBTITLE_FILE_EXTENSIONS.contains(&"sup"));
        assert!(SUBTITLE_FILE_EXTENSIONS.contains(&"idx"));
    }
}
