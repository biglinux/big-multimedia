//! User-facing labels for FFmpeg command fragments and codec identifiers.

/// Return a friendly label for a known FFmpeg video encoder id.
#[must_use]
pub fn video_encoder_label(encoder: &str) -> &str {
    match encoder {
        "libx264" => "H.264 (CPU)",
        "libx265" => "H.265 (CPU)",
        "libsvtav1" => "AV1 (CPU)",
        "libvpx-vp9" => "VP9 (CPU)",
        "prores_ks" => "ProRes",
        "h264_nvenc" => "H.264 NVIDIA",
        "hevc_nvenc" => "H.265 NVIDIA",
        "av1_nvenc" => "AV1 NVIDIA",
        "h264_vaapi" => "H.264 AMD/Intel",
        "hevc_vaapi" => "H.265 AMD/Intel",
        "av1_vaapi" => "AV1 AMD/Intel",
        "h264_qsv" => "H.264 Intel QSV",
        "hevc_qsv" => "H.265 Intel QSV",
        "av1_qsv" => "AV1 Intel QSV",
        "copy" => "Copy",
        other => other,
    }
}

/// Extract `-c:v <encoder>` from an FFmpeg command and map it to a friendly label.
#[must_use]
pub fn video_encoder_label_from_command(command: &str) -> String {
    command
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|window| window[0] == "-c:v")
        .map(|window| video_encoder_label(window[1]).to_string())
        .unwrap_or_default()
}

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
    fn known_video_encoders_map_to_user_labels() {
        assert_eq!(video_encoder_label("libx264"), "H.264 (CPU)");
        assert_eq!(video_encoder_label("h264_nvenc"), "H.264 NVIDIA");
        assert_eq!(video_encoder_label("h264_vaapi"), "H.264 AMD/Intel");
        assert_eq!(video_encoder_label("h264_qsv"), "H.264 Intel QSV");
        assert_eq!(video_encoder_label("copy"), "Copy");
    }

    #[test]
    fn unknown_video_encoder_passthrough() {
        assert_eq!(video_encoder_label("custom_encoder"), "custom_encoder");
    }

    #[test]
    fn command_label_reads_video_encoder_argument() {
        assert_eq!(
            video_encoder_label_from_command(
                "ffmpeg -i input.mp4 -c:v h264_nvenc -c:a aac out.mp4"
            ),
            "H.264 NVIDIA"
        );
        assert_eq!(
            video_encoder_label_from_command("ffmpeg -c:v custom_encoder output.mp4"),
            "custom_encoder"
        );
        assert!(video_encoder_label_from_command("ffmpeg -i input.mp4 output.mp4").is_empty());
    }

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
