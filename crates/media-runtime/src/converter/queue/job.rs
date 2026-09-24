//! Supervised FFmpeg execution. No watcher threads, blocking pipe joins, or
//! process-group signal after PID collection. All diagnostics are sanitized.
use super::{ConversionQueueEntry, QueueMessage};
use crate::converter::ffmpeg::{FfmpegJob, progress};
use crate::error::{Error, Result};
use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessOutputMode, BigSubprocessSpec};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub(crate) fn run_job(
    job: &FfmpegJob,
    entry: &ConversionQueueEntry,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<QueueMessage>,
) -> Result<()> {
    let command = job.build_command();
    let spec = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args(command.get_args())
        .stdout(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_hours(2))
        .build();
    if sender
        .send_blocking(QueueMessage::Started {
            entry_id: entry.id,
            command: "ffmpeg (private paths and credentials omitted)".into(),
        })
        .is_err()
    {
        return Err(Error::Cancelled);
    }
    let mut partial = Vec::new();
    let mut record = Vec::new();
    let duration = job.trim.effective_duration(entry.duration_secs);
    let output = spec
        .run_cancellable_with_stderr(cancel, &mut |chunk| {
            if sender.is_closed() {
                return Err(std::io::Error::other("progress consumer closed"));
            }
            for &byte in chunk {
                if byte != b'\n' {
                    if partial.len() >= 8192 {
                        return Err(std::io::Error::other(
                            "FFmpeg diagnostic line exceeded limit",
                        ));
                    }
                    partial.push(byte);
                    continue;
                }
                let line = String::from_utf8_lossy(&partial);
                if let Some((key, _)) = line.split_once('=') {
                    // Accept only numeric protocol fields, never raw paths/URLs.
                    if matches!(
                        key,
                        "out_time"
                            | "out_time_us"
                            | "out_time_ms"
                            | "frame"
                            | "fps"
                            | "speed"
                            | "total_size"
                            | "bitrate"
                            | "progress"
                    ) {
                        if record.len() + partial.len() + 1 > 32768 {
                            return Err(std::io::Error::other(
                                "FFmpeg progress record exceeded limit",
                            ));
                        }
                        record.extend_from_slice(&partial);
                        record.push(b'\n');
                        if key == "progress" {
                            let _ = progress::parse_progress(
                                std::io::Cursor::new(&record),
                                duration,
                                |snapshot| {
                                    if [
                                        snapshot.time_secs,
                                        snapshot.percent,
                                        snapshot.speed,
                                        snapshot.eta_secs,
                                        snapshot.fps,
                                        snapshot.bitrate_kbps,
                                    ]
                                    .iter()
                                    .all(|n| n.is_finite())
                                    {
                                        // Lossy progress is replaceable; terminal outcomes use the reliable queue path.
                                        let _ = sender.try_send(QueueMessage::Progress {
                                            entry_id: entry.id,
                                            progress_snapshot: snapshot,
                                        });
                                    }
                                },
                            );
                            record.clear();
                        }
                    }
                }
                partial.clear();
            }
            Ok(())
        })
        .map_err(|error| match error {
            BigSubprocessError::Cancelled => Error::Cancelled,
            other => Error::ffmpeg("ffmpeg", None, other.to_string()),
        })?;
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if output.status.success() {
        return Ok(());
    }
    // Use only known static categories; never forward untrusted stderr verbatim.
    let lines: Vec<String> = output
        .stderr_lossy()
        .lines()
        .rev()
        .take(20)
        .map(str::to_owned)
        .collect();
    Err(Error::ffmpeg(
        "ffmpeg",
        output.status.code(),
        friendly_ffmpeg_error(&lines, output.status.code().unwrap_or(-1)),
    ))
}

#[must_use]
pub(crate) fn friendly_ffmpeg_error(lines: &[String], code: i32) -> String {
    use crate::i18n::i18n;

    let joined = lines.join("\n");
    let lower = joined.to_lowercase();

    if lower.contains("no such file or directory") {
        return i18n("Input file not found");
    }
    if lower.contains("permission denied") {
        return i18n("Permission denied — check file permissions");
    }
    if lower.contains("no space left on device") {
        return i18n("Disk full — free up space and try again");
    }
    if lower.contains("invalid data found") {
        return i18n("File is corrupted or has an unsupported format");
    }
    if lower.contains("codec not currently supported")
        || (lower.contains("decoder") && lower.contains("not found"))
    {
        return i18n("Unsupported codec — try a different format");
    }
    if lower.contains("encoder") && lower.contains("not found") {
        return i18n("Required encoder not available — check FFmpeg installation");
    }
    if lower.contains("unknown encoder") || lower.contains("unknown decoder") {
        return i18n("Unknown codec — install required codec support");
    }
    if lower.contains("already exists. overwrite") {
        return i18n("Output file already exists");
    }
    if lower.contains("opening encoder")
        || lower.contains("open encoder")
        || lower.contains("applying encoder options")
    {
        return i18n("Could not convert with these settings. Try another format or quality.");
    }
    if lower.contains("could not open") || lower.contains("failed to open") {
        return i18n("Could not open file — check path and permissions");
    }

    format!("{} {code}", i18n("FFmpeg exited with code"))
}
