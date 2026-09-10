// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Media container compatibility helpers.
//!
//! Provides codec allow-lists for two transports that we cannot assume will
//! re-encode on the fly: an MP4 stream copy, and the Chromecast cast probe.
//! All lookups are case-sensitive and use ffmpeg short codec names.

/// Video codecs that may be stream-copied (`-c:v copy`) into an MP4 container.
pub const MP4_VIDEO_CODECS: &[&str] = &["h264", "hevc", "h265", "mpeg4", "av1"];
/// Audio codecs that may be stream-copied (`-c:a copy`) into an MP4 container.
pub const MP4_AUDIO_CODECS: &[&str] = &["aac", "mp3", "ac3", "eac3", "flac", "opus", "alac"];
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

/// Return `true` if `(codec_type, codec)` survives an MP4 stream copy.
///
/// `codec_type` matches ffprobe's `codec_type` field (`"video"`, `"audio"`,
/// `"subtitle"`, …). Non-AV streams (anything other than `"video"` or
/// `"audio"`) are reported as compatible — they pass through as data tracks.
#[must_use]
pub fn is_mp4_copy_compatible_stream(codec_type: &str, codec: &str) -> bool {
    match codec_type {
        "video" => MP4_VIDEO_CODECS.contains(&codec),
        "audio" => MP4_AUDIO_CODECS.contains(&codec),
        _ => true,
    }
}

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
    fn mp4_copy_compatible_stream_accepts_supported_mp4_audio_and_video_codecs() {
        assert!(is_mp4_copy_compatible_stream("video", "h264"));
        assert!(is_mp4_copy_compatible_stream("video", "hevc"));
        assert!(is_mp4_copy_compatible_stream("audio", "aac"));
        assert!(is_mp4_copy_compatible_stream("audio", "opus"));
    }

    #[test]
    fn mp4_copy_compatible_stream_rejects_container_mismatches() {
        assert!(!is_mp4_copy_compatible_stream("video", "vp9"));
        assert!(!is_mp4_copy_compatible_stream("audio", "vorbis"));
    }

    #[test]
    fn mp4_copy_compatible_stream_ignores_non_av_streams() {
        assert!(is_mp4_copy_compatible_stream("subtitle", "subrip"));
        assert!(is_mp4_copy_compatible_stream("data", "bin_data"));
    }

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
