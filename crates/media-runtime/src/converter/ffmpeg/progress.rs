//! FFmpeg progress wrappers and runtime-side `ffprobe` helpers.

use std::io::BufRead;
use std::time::Duration;

use big_media_engines::ffmpeg::progress as canonical_progress;
use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

/// One serializable snapshot of FFmpeg progress derived from `-progress pipe:2`.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct FfmpegProgressSnapshot {
    pub time_secs: f64,
    pub total_secs: f64,
    pub speed: f64,
    pub size_bytes: u64,
    pub bitrate_kbps: f64,
    pub frame: u64,
    pub fps: f64,
    pub eta_secs: f64,
    pub percent: f64,
    pub done: bool,
}

impl Default for FfmpegProgressSnapshot {
    fn default() -> Self {
        Self {
            time_secs: 0.0,
            total_secs: 0.0,
            speed: 0.0,
            size_bytes: 0,
            bitrate_kbps: 0.0,
            frame: 0,
            fps: 0.0,
            eta_secs: 0.0,
            percent: 0.0,
            done: false,
        }
    }
}

impl From<canonical_progress::FfmpegProgressSnapshot> for FfmpegProgressSnapshot {
    fn from(snapshot: canonical_progress::FfmpegProgressSnapshot) -> Self {
        Self {
            time_secs: snapshot.time_secs,
            total_secs: snapshot.total_secs,
            speed: snapshot.speed,
            size_bytes: snapshot.size_bytes,
            bitrate_kbps: snapshot.bitrate_kbps,
            frame: snapshot.frame,
            fps: snapshot.fps,
            eta_secs: snapshot.eta_secs,
            percent: snapshot.percent,
            done: snapshot.done,
        }
    }
}

/// Parse FFmpeg `-progress pipe:2` lines from `reader`, emitting snapshots.
pub fn parse_progress<R: BufRead, F: FnMut(FfmpegProgressSnapshot)>(
    reader: R,
    total_duration_secs: f64,
    mut on_progress: F,
) -> Vec<String> {
    canonical_progress::parse_progress(reader, total_duration_secs, |snapshot| {
        on_progress(snapshot.into());
    })
}

/// Like [`parse_progress`] but forwards every stderr line to `on_stderr`.
pub fn parse_progress_live<R, F, S>(
    reader: R,
    total_duration_secs: f64,
    mut on_progress: F,
    on_stderr: S,
) -> Vec<String>
where
    R: BufRead,
    F: FnMut(FfmpegProgressSnapshot),
    S: FnMut(&str),
{
    canonical_progress::parse_progress_live(
        reader,
        total_duration_secs,
        |snapshot| on_progress(snapshot.into()),
        on_stderr,
    )
}

/// Probe `path` with `ffprobe` and return its duration in seconds, `0.0` on failure.
///
/// `ffprobe` is invoked with `--` to terminate option parsing, so a path
/// starting with `-` cannot be mistaken for a flag.
pub fn probe_duration(path: &std::path::Path) -> f64 {
    let output = BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "quiet",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            "--",
        ])
        .arg(path)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(10))
        .build()
        .run();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            text.trim().parse().unwrap_or(0.0)
        }
        _ => 0.0,
    }
}

/// Probe `path` with `ffprobe` and return the first audio stream channel count.
///
/// Falls back to `2` if `ffprobe` cannot be invoked, the file has no audio, or
/// the output cannot be parsed.
pub fn probe_audio_channels(path: &std::path::Path) -> u32 {
    let output = BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "quiet",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=channels",
            "-of",
            "csv=p=0",
            "--",
        ])
        .arg(path)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(10))
        .build()
        .run();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            text.trim().parse().unwrap_or(2)
        }
        _ => 2,
    }
}

/// Verify terminal ffprobe success; cancellation is not an empty metadata record.
pub fn probe_media_metadata_cancellable(
    path: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> crate::Result<serde_json::Value> {
    let output = BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "--",
        ])
        .arg(path)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(10))
        .build()
        .run_cancellable(cancel)
        .map_err(|error| match error {
            big_os_kit::subprocess::BigSubprocessError::Cancelled => crate::Error::Cancelled,
            _ => crate::Error::internal("Media probe did not complete"),
        })?;
    if !output.status.success() {
        return Err(crate::Error::internal("Media probe failed"));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| crate::Error::internal("Invalid media probe response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_progress_wraps_canonical_snapshot_and_preserves_total_secs() {
        let progress_lines = "\
frame=100\n\
out_time=00:00:10.000\n\
progress=continue\n\
progress=end\n";
        let mut updates = Vec::new();

        let error_lines = parse_progress(Cursor::new(progress_lines), 20.0, |snapshot| {
            updates.push(snapshot);
        });

        assert!(error_lines.is_empty());
        assert_eq!(updates[0].frame, 100);
        assert!((updates[0].time_secs - 10.0).abs() < 0.001);
        assert!((updates[0].total_secs - 20.0).abs() < f64::EPSILON);
        assert!((updates[0].percent - 50.0).abs() < 1.0);
        assert!(updates.last().is_some_and(|snapshot| snapshot.done));
    }

    #[test]
    fn parse_progress_live_forwards_stderr_lines() {
        let progress_lines = "\
Warning: stream has no timestamps\n\
out_time=00:00:01.000\n\
progress=continue\n";
        let mut updates = Vec::new();
        let mut stderr_lines = Vec::new();

        let collected = parse_progress_live(
            Cursor::new(progress_lines),
            2.0,
            |snapshot| updates.push(snapshot),
            |line| stderr_lines.push(line.to_owned()),
        );

        assert_eq!(stderr_lines, vec!["Warning: stream has no timestamps"]);
        assert_eq!(collected, stderr_lines);
        assert_eq!(updates.len(), 1);
    }

    #[test]
    fn progress_snapshot_serializes_for_runtime_journals() {
        let snapshot = FfmpegProgressSnapshot {
            time_secs: 1.5,
            total_secs: 3.0,
            percent: 50.0,
            ..Default::default()
        };

        let encoded = serde_json::to_string(&snapshot).expect("serialize");
        let decoded: FfmpegProgressSnapshot = serde_json::from_str(&encoded).expect("deserialize");

        assert_eq!(decoded, snapshot);
    }
}
