//! Headless end-to-end smoke for the in-player quick-export runner.
//!
//! Generates a tiny A/V clip with the system ffmpeg, then drives
//! `run_single_export_blocking` for the two player paths (extract audio,
//! convert video) and asserts a non-empty output file lands. `#[ignore]` —
//! requires a working ffmpeg, so it runs on demand, not in the unit gate:
//!
//! ```sh
//! cargo test -p big-quick-export --test export_smoke -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use big_media_runtime::converter::profiles::VideoFilters;
use big_quick_export::export::{QuickExportChoice, QuickExportFormat, run_single_export_blocking};

fn make_clip(dir: &Path) -> PathBuf {
    let clip = dir.join("clip.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=24:duration=3",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
        .args(["-shortest", "-pix_fmt", "yuv420p"])
        .arg(&clip)
        .status()
        .expect("spawn ffmpeg to build the test clip");
    assert!(status.success(), "ffmpeg failed to build the test clip");
    clip
}

fn assert_non_empty(path: &Path) {
    let meta = std::fs::metadata(path).unwrap_or_else(|e| panic!("no output at {path:?}: {e}"));
    assert!(meta.len() > 0, "output {path:?} is empty");
}

#[test]
#[ignore = "requires system ffmpeg; run with --ignored"]
fn extract_audio_to_mp3_produces_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let clip = make_clip(dir.path());
    let out = run_single_export_blocking(
        clip,
        dir.path().to_path_buf(),
        &QuickExportChoice {
            format: QuickExportFormat::Audio {
                codec: "mp3".into(),
                bitrate: "128k".into(),
            },
            filters: VideoFilters::default(),
        },
        &[],
    )
    .expect("extract-audio export should succeed");
    assert_eq!(out.extension().and_then(|e| e.to_str()), Some("mp3"));
    assert_non_empty(&out);
}

#[test]
#[ignore = "requires system ffmpeg; run with --ignored"]
fn convert_and_trim_video_produces_a_clip() {
    let dir = tempfile::tempdir().unwrap();
    let clip = make_clip(dir.path());
    let out = run_single_export_blocking(
        clip,
        dir.path().to_path_buf(),
        &QuickExportChoice {
            format: QuickExportFormat::Video {
                container: "mp4".into(),
                codec: "h264".into(),
                quality: "good".into(),
            },
            filters: VideoFilters::default(),
        },
        // Keep only seconds 1..2 of the 3s clip.
        &[(1.0, 2.0)],
    )
    .expect("convert+trim export should succeed");
    assert_eq!(out.extension().and_then(|e| e.to_str()), Some("mp4"));
    assert_non_empty(&out);
}
