use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

use serde_json::json;

use super::super::detailed_video_metadata::{
    parse_audio_stream, parse_subtitle_stream, parse_video_stream, probe_detailed_video_metadata,
};

static PROBE_ENV_LOCK: Mutex<()> = Mutex::new(());

struct PathOverride {
    previous_path: Option<OsString>,
}

impl PathOverride {
    fn prepend(directory: &std::path::Path) -> Self {
        let previous_path = std::env::var_os("PATH");
        let mut path_entries = vec![directory.to_path_buf()];
        if let Some(path) = previous_path.clone() {
            path_entries.extend(std::env::split_paths(&path));
        }
        let joined_path =
            std::env::join_paths(path_entries).expect("temporary ffprobe PATH should be valid");
        // SAFETY: guarded by PROBE_ENV_LOCK and restored in Drop before the test returns.
        unsafe {
            std::env::set_var("PATH", joined_path);
        }
        Self { previous_path }
    }
}

impl Drop for PathOverride {
    fn drop(&mut self) {
        // SAFETY: guarded by PROBE_ENV_LOCK; restores process PATH to the prior value.
        unsafe {
            if let Some(path) = &self.previous_path {
                std::env::set_var("PATH", path);
            } else {
                std::env::remove_var("PATH");
            }
        }
    }
}

fn install_fake_ffprobe(directory: &std::path::Path, exit_status: i32, stdout_json: &str) {
    let script_path = directory.join("ffprobe");
    fs::write(
        &script_path,
        format!("#!/bin/sh\ncat <<'JSON'\n{stdout_json}\nJSON\nexit {exit_status}\n"),
    )
    .expect("fake ffprobe script should be writable");
    let mut permissions = fs::metadata(&script_path)
        .expect("fake ffprobe script metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(script_path, permissions)
        .expect("fake ffprobe script should be executable");
}

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
fn parse_video_stream_ignores_zero_denominator_frame_rate() {
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
fn parse_audio_stream_formats_channels_quality_and_title() {
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
fn probe_detailed_video_metadata_reads_file_fields_and_groups_streams() {
    let _environment_guard = PROBE_ENV_LOCK
        .lock()
        .expect("probe env lock should not be poisoned");
    let temporary_directory = tempfile::Builder::new()
        .prefix("metadata-cache-test-")
        .tempdir_in(std::env::current_dir().expect("crate directory should be readable"))
        .expect("temporary directory should be created");
    let media_path = temporary_directory.path().join("sample.mkv");
    fs::write(&media_path, vec![0_u8; 2048]).expect("temporary media file should be writable");
    install_fake_ffprobe(
        temporary_directory.path(),
        0,
        r#"{
            "format": {
                "duration": "3723.456",
                "format_long_name": "Matroska / WebM",
                "bit_rate": "9876543"
            },
            "streams": [
                {
                    "codec_type": "video",
                    "codec_name": "h264",
                    "profile": "High",
                    "width": 1280,
                    "height": 720,
                    "r_frame_rate": "24000/1001",
                    "pix_fmt": "yuv420p",
                    "bit_rate": "2500000",
                    "tags": { "language": "eng" }
                },
                {
                    "codec_type": "audio",
                    "codec_name": "aac",
                    "sample_rate": "44100",
                    "channels": 2,
                    "tags": { "language": "por" }
                },
                {
                    "codec_type": "subtitle",
                    "codec_name": "subrip",
                    "tags": { "language": "spa" }
                }
            ]
        }"#,
    );
    let _path_override = PathOverride::prepend(temporary_directory.path());
    let fake_output = std::process::Command::new("ffprobe")
        .output()
        .expect("fake ffprobe should run through PATH");
    assert!(
        fake_output.status.success(),
        "fake ffprobe should exit successfully: stderr={}",
        String::from_utf8_lossy(&fake_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&fake_output.stdout).contains("Matroska / WebM"),
        "fake ffprobe stdout should contain fixture JSON"
    );

    let metadata = probe_detailed_video_metadata(
        media_path
            .to_str()
            .expect("temporary media path should be valid UTF-8"),
    )
    .expect("fake ffprobe success should produce metadata");

    assert_eq!(metadata.file_name, "sample.mkv");
    assert_eq!(
        metadata.file_dir,
        temporary_directory.path().to_string_lossy()
    );
    assert_eq!(metadata.file_size.as_deref(), Some("2.0 KB"));
    assert_eq!(metadata.duration.as_deref(), Some("01:02:03.456"));
    assert_eq!(metadata.format_name.as_deref(), Some("Matroska / WebM"));
    assert_eq!(metadata.bitrate.as_deref(), Some("9876.54 kbps"));
    assert_eq!(metadata.video_streams.len(), 1);
    assert_eq!(metadata.audio_streams.len(), 1);
    assert_eq!(metadata.subtitle_streams.len(), 1);
    assert!(
        metadata.video_streams[0]
            .iter()
            .any(|(k, v)| k == "Resolution" && v == "1280×720")
    );
    assert!(
        metadata.audio_streams[0]
            .iter()
            .any(|(k, v)| k == "Sample Rate" && v == "44100 Hz (CD Quality)")
    );
    assert!(
        metadata.subtitle_streams[0]
            .iter()
            .any(|(k, v)| k == "Language" && v == "SPA")
    );
}

#[test]
fn probe_detailed_video_metadata_rejects_failed_ffprobe() {
    let _environment_guard = PROBE_ENV_LOCK
        .lock()
        .expect("probe env lock should not be poisoned");
    let temporary_directory = tempfile::Builder::new()
        .prefix("metadata-cache-test-")
        .tempdir_in(std::env::current_dir().expect("crate directory should be readable"))
        .expect("temporary directory should be created");
    let media_path = temporary_directory.path().join("failed.mkv");
    fs::write(&media_path, [0_u8]).expect("temporary media file should be writable");
    install_fake_ffprobe(
        temporary_directory.path(),
        2,
        r#"{"format":{"duration":"1.000"},"streams":[]}"#,
    );
    let _path_override = PathOverride::prepend(temporary_directory.path());

    assert!(
        probe_detailed_video_metadata(
            media_path
                .to_str()
                .expect("temporary media path should be valid UTF-8")
        )
        .is_none(),
        "failed ffprobe status must not produce metadata"
    );
}
