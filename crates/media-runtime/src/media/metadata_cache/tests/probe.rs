use super::super::probe::{codec_display_name, parse_ffprobe_json};

#[test]
fn parse_ffprobe_basic_video() {
    let json = r#"{
        "format": { "duration": "123.456" },
        "streams": [
            { "codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080 },
            { "codec_type": "audio" }
        ]
    }"#;
    let meta = parse_ffprobe_json(json).unwrap();
    assert!((meta.duration_secs.unwrap() - 123.456).abs() < 0.001);
    assert_eq!(meta.resolution.as_deref(), Some("1920×1080"));
    assert_eq!(meta.codec.as_deref(), Some("H.264"));
    assert_eq!(meta.audio_tracks, 1);
    assert_eq!(meta.subtitle_tracks, 0);
}

#[test]
fn parse_ffprobe_multiple_streams() {
    let json = r#"{
        "format": { "duration": "60.0" },
        "streams": [
            { "codec_type": "video", "codec_name": "hevc", "width": 3840, "height": 2160 },
            { "codec_type": "audio" },
            { "codec_type": "audio" },
            { "codec_type": "subtitle" },
            { "codec_type": "subtitle" },
            { "codec_type": "subtitle" }
        ]
    }"#;
    let meta = parse_ffprobe_json(json).unwrap();
    assert_eq!(meta.codec.as_deref(), Some("H.265"));
    assert_eq!(meta.resolution.as_deref(), Some("3840×2160"));
    assert_eq!(meta.audio_tracks, 2);
    assert_eq!(meta.subtitle_tracks, 3);
}

#[test]
fn parse_ffprobe_no_streams() {
    let json = r#"{ "format": { "duration": "10.0" } }"#;
    let meta = parse_ffprobe_json(json).unwrap();
    assert!(meta.duration_secs.is_some());
    assert!(meta.resolution.is_none());
    assert!(meta.codec.is_none());
}

#[test]
fn parse_ffprobe_no_duration() {
    let json = r#"{
        "format": {},
        "streams": [{ "codec_type": "video", "codec_name": "av1", "width": 1280, "height": 720 }]
    }"#;
    let meta = parse_ffprobe_json(json).unwrap();
    assert!(meta.duration_secs.is_none());
    assert_eq!(meta.codec.as_deref(), Some("AV1"));
}

#[test]
fn parse_ffprobe_invalid_json() {
    assert!(parse_ffprobe_json("not json").is_none());
}

#[test]
fn parse_ffprobe_zero_dimensions_skipped() {
    let json = r#"{
        "format": {},
        "streams": [{ "codec_type": "video", "codec_name": "h264", "width": 0, "height": 0 }]
    }"#;
    let meta = parse_ffprobe_json(json).unwrap();
    assert!(meta.resolution.is_none());
}

#[test]
fn parse_ffprobe_first_video_stream_wins() {
    let json = r#"{
        "format": {},
        "streams": [
            { "codec_type": "video", "codec_name": "h264", "width": 1920, "height": 1080 },
            { "codec_type": "video", "codec_name": "vp9", "width": 1280, "height": 720 }
        ]
    }"#;
    let meta = parse_ffprobe_json(json).unwrap();
    assert_eq!(meta.resolution.as_deref(), Some("1920×1080"));
    assert_eq!(meta.codec.as_deref(), Some("H.264"));
}

#[test]
fn codec_display_known_names() {
    assert_eq!(codec_display_name("h264"), "H.264");
    assert_eq!(codec_display_name("hevc"), "H.265");
    assert_eq!(codec_display_name("h265"), "H.265");
    assert_eq!(codec_display_name("av1"), "AV1");
    assert_eq!(codec_display_name("vp9"), "VP9");
    assert_eq!(codec_display_name("vp8"), "VP8");
    assert_eq!(codec_display_name("mpeg4"), "MPEG-4");
    assert_eq!(codec_display_name("mpeg2video"), "MPEG-2");
    assert_eq!(codec_display_name("prores"), "ProRes");
    assert_eq!(codec_display_name("dnxhd"), "DNxHD");
    assert_eq!(codec_display_name("theora"), "Theora");
}

#[test]
fn codec_display_unknown_uppercased() {
    assert_eq!(codec_display_name("rawvideo"), "RAWVIDEO");
    assert_eq!(codec_display_name("flv1"), "FLV1");
}

#[test]
fn codec_display_lib_prefixed() {
    assert_eq!(codec_display_name("libx264"), "H.264");
    assert_eq!(codec_display_name("libx265"), "H.265");
    assert_eq!(codec_display_name("libaom-av1"), "AV1");
    assert_eq!(codec_display_name("libsvtav1"), "AV1");
    assert_eq!(codec_display_name("libvpx-vp9"), "VP9");
    assert_eq!(codec_display_name("libvpx"), "VP8");
}
