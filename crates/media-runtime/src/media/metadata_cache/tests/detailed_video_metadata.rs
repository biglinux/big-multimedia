use std::fs;

use serde_json::json;

use super::super::detailed_video_metadata::{
    detailed_video_metadata, parse_audio_stream, parse_subtitle_stream, parse_video_stream,
};

#[test]
fn parse_video_stream_formats_known_fields() {
    let stream = json!({
        "codec_name": "h264",
        "profile": "High",
        "width": 1920,
        "height": 1080,
        "r_frame_rate": "30000/1001",
        "pix_fmt": "yuv420p",
        "bit_rate": "4500000",
        "tags": { "language": "eng" }
    });

    let fields = parse_video_stream(&stream);
    assert!(
        fields
            .iter()
            .any(|(k, v)| k == "Codec" && v == "H.264 (High)")
    );
    assert!(
        fields
            .iter()
            .any(|(k, v)| k == "Resolution" && v == "1920×1080")
    );
    assert!(
        fields
            .iter()
            .any(|(k, v)| k == "Frame Rate" && v.contains("29.970"))
    );
    assert!(
        fields
            .iter()
            .any(|(k, v)| k == "Pixel Format" && v == "yuv420p")
    );
    assert!(
        fields
            .iter()
            .any(|(k, v)| k == "Bitrate" && v == "4500.00 kbps")
    );
    assert!(fields.iter().any(|(k, v)| k == "Language" && v == "ENG"));
}

#[test]
fn zero_denominator_frame_rate_ignored() {
    let stream = json!({
        "codec_name": "h264",
        "r_frame_rate": "30000/0"
    });

    let fields = parse_video_stream(&stream);
    assert!(
        !fields.iter().any(|(k, _)| k == "Frame Rate"),
        "zero denominator frame rate must not be displayed"
    );
}

#[test]
fn audio_stream_fields_are_formatted() {
    let stream = json!({
        "codec_name": "aac",
        "profile": "LC",
        "sample_rate": "48000",
        "channels": 6,
        "bit_rate": "384000",
        "tags": { "language": "por", "title": "Main Mix" }
    });

    let fields = parse_audio_stream(&stream);
    assert!(fields.iter().any(|(k, v)| k == "Codec" && v == "AAC (LC)"));
    assert!(
        fields
            .iter()
            .any(|(k, v)| k == "Sample Rate" && v == "48000 Hz (Hi-Res)")
    );
    assert!(
        fields
            .iter()
            .any(|(k, v)| k == "Channels" && v == "6 (5.1 Surround)")
    );
    assert!(
        fields
            .iter()
            .any(|(k, v)| k == "Bitrate" && v == "384.00 kbps")
    );
    assert!(fields.iter().any(|(k, v)| k == "Language" && v == "POR"));
    assert!(fields.iter().any(|(k, v)| k == "Title" && v == "Main Mix"));
}

#[test]
fn parse_subtitle_stream_formats_basic_fields() {
    let stream = json!({
        "codec_name": "subrip",
        "tags": { "language": "spa", "title": "Latin America" }
    });

    let fields = parse_subtitle_stream(&stream);
    assert!(fields.iter().any(|(k, v)| k == "Codec" && v == "SUBRIP"));
    assert!(fields.iter().any(|(k, v)| k == "Language" && v == "SPA"));
    assert!(
        fields
            .iter()
            .any(|(k, v)| k == "Title" && v == "Latin America")
    );
}

#[test]
fn report_groups_streams_by_kind() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let media_path = directory.path().join("sample.mkv");
    fs::write(&media_path, vec![0_u8; 2048]).expect("temporary media file should be writable");
    let report = json!({
        "format": {
            "duration": "3723.456",
            "format_long_name": "Matroska / WebM",
            "bit_rate": "9876543"
        },
        "streams": [
            {
                "codec_type": "video",
                "codec_name": "h264",
                "width": 1280,
                "height": 720
            },
            {
                "codec_type": "audio",
                "codec_name": "aac",
                "sample_rate": "44100",
                "channels": 2
            },
            {
                "codec_type": "subtitle",
                "codec_name": "subrip",
                "tags": { "language": "spa" }
            }
        ]
    });

    let metadata = detailed_video_metadata(
        media_path
            .to_str()
            .expect("temporary media path should be valid UTF-8"),
        &report,
    );

    assert_eq!(metadata.file_name, "sample.mkv");
    assert_eq!(metadata.file_dir, directory.path().to_string_lossy());
    assert_eq!(metadata.file_size.as_deref(), Some("2.0 KB"));
    assert_eq!(metadata.duration.as_deref(), Some("01:02:03.456"));
    assert_eq!(metadata.format_name.as_deref(), Some("Matroska / WebM"));
    assert_eq!(metadata.bitrate.as_deref(), Some("9876.54 kbps"));
    assert_eq!(metadata.video_streams.len(), 1);
    assert_eq!(metadata.audio_streams.len(), 1);
    assert_eq!(metadata.subtitle_streams.len(), 1);
    assert!(
        metadata.audio_streams[0]
            .iter()
            .any(|(k, v)| k == "Sample Rate" && v == "44100 Hz (CD Quality)")
    );
}
