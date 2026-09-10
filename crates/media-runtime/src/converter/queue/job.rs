//! Single-job runner: spawn ffmpeg in its own process group (setsid so
//! cancel kills the whole tree, including grandchildren), watcher thread
//! enforces 2h hard timeout + cancel signal (SIGTERM, then SIGKILL after
//! 3s), live stderr is forwarded as both progress events and individual
//! lines, and `friendly_ffmpeg_error` translates known stderr fingerprints
//! into user-facing pt-BR messages.

use crate::converter::ffmpeg::FfmpegJob;
use crate::converter::ffmpeg::progress;
use crate::error::{Error, Result};
use std::io::BufReader;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{ConversionQueueEntry, QueueMessage};

pub(crate) fn run_job(
    job: &FfmpegJob,
    queue_entry: &ConversionQueueEntry,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<QueueMessage>,
) -> Result<()> {
    let mut cmd = job.build_command();
    cmd.stderr(Stdio::piped());
    cmd.stdout(Stdio::null());

    let cmd_display = format!("{cmd:?}");
    let _ = sender.send_blocking(QueueMessage::Started {
        entry_id: queue_entry.id,
        command: cmd_display,
    });

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: pre_exec runs after fork, setsid creates new process group
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::ffmpeg("ffmpeg", None, format!("spawn: {e}")))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::ffmpeg("ffmpeg", None, "no stderr pipe"))?;

    let child_pid = child.id();
    let cancel_arc = Arc::new(AtomicBool::new(false));
    let cancel_watcher = cancel_arc.clone();
    let cancel_flag = cancel.clone();
    let watcher_handle = std::thread::Builder::new()
        .name("cancel-watcher".into())
        .spawn(move || {
            const TIMEOUT_MS: u64 = 2 * 60 * 60 * 1000;
            let mut elapsed_ms: u64 = 0;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                elapsed_ms += 100;
                let should_kill = cancel_flag.load(Ordering::Relaxed) || elapsed_ms >= TIMEOUT_MS;
                if should_kill {
                    if elapsed_ms >= TIMEOUT_MS {
                        log::warn!("Conversion timeout reached (2h), killing process");
                    }
                    #[cfg(unix)]
                    {
                        // SAFETY: negative pid targets the process group created via setsid.
                        unsafe {
                            libc::kill(-(child_pid as i32), libc::SIGTERM);
                        }
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        // SAFETY: same process group semantics as above.
                        unsafe {
                            libc::kill(-(child_pid as i32), libc::SIGKILL);
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = child_pid;
                    }
                    cancel_watcher.store(true, Ordering::Relaxed);
                    return;
                }
                if cancel_watcher.load(Ordering::Relaxed) {
                    return;
                }
            }
        })
        .ok();

    let reader = BufReader::new(stderr);
    let entry_id = queue_entry.id;
    let sender_for_progress = sender.clone();
    let cancel_for_progress = cancel.clone();

    // Percent must track THIS job's processed span, not the whole media: when a
    // trim/cut is active ffmpeg only encodes `[start, end]` (input `-ss` + `-t`),
    // so a 5%-long cut would otherwise read as 100% and "finish" at 5%.
    let progress_duration = job.trim.effective_duration(queue_entry.duration_secs);
    let error_lines = progress::parse_progress_live(
        reader,
        progress_duration,
        move |progress_snapshot| {
            if cancel_for_progress.load(Ordering::Relaxed) {
                return;
            }
            let _ = sender_for_progress.send_blocking(QueueMessage::Progress {
                entry_id,
                progress_snapshot,
            });
        },
        {
            let sender_for_lines = sender.clone();
            move |line: &str| {
                let _ = sender_for_lines.send_blocking(QueueMessage::StderrLine {
                    entry_id,
                    line: line.to_string(),
                });
            }
        },
    );

    if !error_lines.is_empty() {
        let _ = sender.send_blocking(QueueMessage::StderrLog {
            entry_id,
            lines: error_lines.clone(),
        });
    }

    cancel_arc.store(true, Ordering::Relaxed);
    if let Some(handle) = watcher_handle {
        let _ = handle.join();
    }

    if cancel.load(Ordering::Relaxed) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::Cancelled);
    }

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        let code = status.code();
        Err(Error::ffmpeg(
            "ffmpeg",
            code,
            friendly_ffmpeg_error(&error_lines, code.unwrap_or(-1)),
        ))
    }
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
    if lower.contains("could not open") || lower.contains("failed to open") {
        return i18n("Could not open file — check path and permissions");
    }

    if let Some(last) = lines.iter().rev().find(|line| {
        let lower_line = line.to_lowercase();
        lower_line.contains("error")
            || lower_line.contains("invalid")
            || lower_line.contains("failed")
    }) {
        return last.clone();
    }

    format!("{} {code}", i18n("FFmpeg exited with code"))
}
