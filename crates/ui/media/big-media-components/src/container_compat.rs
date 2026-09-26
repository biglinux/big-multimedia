// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Media container compatibility helpers.
//!
//! Provides codec allow-lists for two transports that we cannot assume will
//! re-encode on the fly: an MP4 stream copy, and the Chromecast cast probe.
//! All lookups are case-sensitive and use ffmpeg short codec names.

/// Video codecs the Chromecast probe accepts without re-encoding.
pub const CHROMECAST_VIDEO_CODECS: &[&str] = &["h264", "hevc", "vp8", "vp9", "av1"];
/// Audio codecs the Chromecast probe accepts without re-encoding.
pub const CHROMECAST_AUDIO_CODECS: &[&str] = &[
    "aac",
    "mp3",
    "opus",
    "vorbis",
    "flac",
    "pcm_s16le",
    "pcm_s24le",
];

/// Return `true` if `codec` is in [`CHROMECAST_VIDEO_CODECS`].
#[must_use]
pub fn is_chromecast_video_codec(codec: &str) -> bool {
    CHROMECAST_VIDEO_CODECS.contains(&codec)
}

/// Return `true` if `codec` is in [`CHROMECAST_AUDIO_CODECS`].
#[must_use]
pub fn is_chromecast_audio_codec(codec: &str) -> bool {
    CHROMECAST_AUDIO_CODECS.contains(&codec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromecast_codec_sets_match_cast_probe_contract() {
        assert!(is_chromecast_video_codec("h264"));
        assert!(is_chromecast_video_codec("vp9"));
        assert!(!is_chromecast_video_codec("mpeg2video"));
        assert!(is_chromecast_audio_codec("aac"));
        assert!(is_chromecast_audio_codec("pcm_s24le"));
        assert!(!is_chromecast_audio_codec("dts"));
    }
}
