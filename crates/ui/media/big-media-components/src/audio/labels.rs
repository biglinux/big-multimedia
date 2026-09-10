// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared audio metadata label formatting.
//!
//! Format ffprobe-style audio metadata into a single line for queue rows
//! and headerbars. Empty or non-positive values are omitted; nothing
//! allocates beyond the returned `String`.

/// Format an audio summary line, e.g. `"FLAC 48kHz 320kbps stereo"`.
///
/// `codec` is uppercased; `sample_rate` is shown in kHz with one decimal
/// for non-multiples of 1000; `bitrate` is shown in kbps; `channels` is
/// rendered verbatim. Missing parts (`""`, `0`) are silently skipped.
#[must_use]
pub fn audio_metadata_summary_label(
    codec: &str,
    sample_rate: i64,
    bitrate: i64,
    channels: &str,
) -> String {
    let mut parts = Vec::new();
    if !codec.is_empty() {
        parts.push(codec.to_uppercase());
    }
    if sample_rate > 0 {
        if sample_rate % 1000 == 0 {
            parts.push(format!("{}kHz", sample_rate / 1000));
        } else {
            let khz = sample_rate as f64 / 1000.0;
            parts.push(format!("{khz:.1}kHz"));
        }
    }
    if bitrate > 0 {
        parts.push(format!("{}kbps", bitrate / 1000));
    }
    if !channels.is_empty() {
        parts.push(channels.to_string());
    }
    parts.join(" ")
}

/// Format `bitrate` (bits per second) as a `"<value> kbps"` label.
#[must_use]
pub fn bitrate_kbps_label_from_bps(bitrate: i64) -> String {
    format!("{} kbps", bitrate / 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_metadata_summary_label_formats_full_metadata() {
        assert_eq!(
            audio_metadata_summary_label("flac", 48_000, 320_000, "stereo"),
            "FLAC 48kHz 320kbps stereo"
        );
    }

    #[test]
    fn audio_metadata_summary_label_handles_fractional_sample_rate() {
        assert_eq!(
            audio_metadata_summary_label("mp3", 44_100, 128_000, "stereo"),
            "MP3 44.1kHz 128kbps stereo"
        );
    }

    #[test]
    fn audio_metadata_summary_label_omits_missing_metadata_parts() {
        assert_eq!(audio_metadata_summary_label("", 0, 0, ""), "");
        assert_eq!(audio_metadata_summary_label("vorbis", 0, 0, ""), "VORBIS");
        assert_eq!(
            audio_metadata_summary_label("aac", 44_100, 256_000, ""),
            "AAC 44.1kHz 256kbps"
        );
    }

    #[test]
    fn bitrate_kbps_label_uses_decimal_bps() {
        assert_eq!(bitrate_kbps_label_from_bps(128_000), "128 kbps");
        assert_eq!(bitrate_kbps_label_from_bps(0), "0 kbps");
    }
}
