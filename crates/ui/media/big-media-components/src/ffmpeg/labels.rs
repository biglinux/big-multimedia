//! User-facing labels for FFmpeg command fragments and codec identifiers.

/// Return a compact display name for media codec ids reported by FFmpeg/ffprobe.
#[must_use]
pub fn media_codec_display_name(name: &str) -> String {
    match name {
        "h264" | "libx264" => "H.264".into(),
        "hevc" | "h265" | "libx265" => "H.265".into(),
        "av1" | "libaom-av1" | "libsvtav1" => "AV1".into(),
        "vp9" | "libvpx-vp9" => "VP9".into(),
        "vp8" | "libvpx" => "VP8".into(),
        "mpeg4" => "MPEG-4".into(),
        "mpeg2video" => "MPEG-2".into(),
        "prores" => "ProRes".into(),
        "dnxhd" => "DNxHD".into(),
        "theora" => "Theora".into(),
        other => other.to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_codec_display_names_cover_recognized_video_encoder_names() {
        assert_eq!(media_codec_display_name("h264"), "H.264");
        assert_eq!(media_codec_display_name("libx265"), "H.265");
        assert_eq!(media_codec_display_name("libsvtav1"), "AV1");
        assert_eq!(media_codec_display_name("libvpx-vp9"), "VP9");
        assert_eq!(media_codec_display_name("mpeg2video"), "MPEG-2");
        assert_eq!(media_codec_display_name("rawvideo"), "RAWVIDEO");
    }
}
