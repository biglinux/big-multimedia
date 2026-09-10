//! Queue helpers: output-path dedup ("_converted", "_converted_N"),
//! per-segment naming for split mode, free-disk probe (statvfs on Unix,
//! None elsewhere), human-readable byte sizes, and the HEVC-10bit→H.264
//! shortcut detector that lets the GPU stage skip the SW filter chain.

use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) use big_media_engines::output_paths::{dedup_output_path, segment_output_path};
use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

use super::ConversionQueueEntry;
use crate::converter::ffmpeg::progress;

/// Guarantee an queue_entry's probe-derived fields are real before it is converted.
///
/// `add_file` hands probing to a background worker and pushes the queue_entry with
/// safe defaults (`duration_secs = 0`, `input_audio_channels = 0`,
/// `skip_video_filters = false`). If the user starts the queue before that
/// worker's `Probed` message is drained, `Queue::start` snapshots the queue_entry
/// while it is still `!probed` — converting with stale metadata (wrong audio
/// downmix, missing HEVC-10bit→H.264 filter skip, no progress %).
///
/// This runs on the converter worker thread (never the GTK main loop), so the
/// inline ffprobe is free to block. No-op once `probed` is set, so a normally
/// pre-probed queue_entry pays nothing.
pub(crate) fn ensure_entry_probed(queue_entry: &mut ConversionQueueEntry, target_codec: &str) {
    if queue_entry.probed {
        return;
    }
    queue_entry.duration_secs = progress::probe_duration(&queue_entry.input);
    queue_entry.input_audio_channels = progress::probe_audio_channels(&queue_entry.input);
    queue_entry.skip_video_filters = detect_hevc_10bit_to_h264(&queue_entry.input, target_codec);
    queue_entry.probed = true;
}

#[must_use]
pub(crate) fn resume_partial_output_path(
    final_output: &Path,
    job_key: &str,
    entry_id: u64,
    segment_idx: usize,
) -> PathBuf {
    let dir = final_output.parent().unwrap_or(Path::new("."));
    let ext = final_output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mp4");
    let token = stable_resume_token(job_key);
    dir.join(format!(
        ".big-convertd-{token}-i{entry_id}-s{segment_idx}.part.{ext}"
    ))
}

#[must_use]
pub(crate) fn is_resume_output_complete(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

pub(crate) fn promote_resume_output(
    partial_output: &Path,
    final_output: &Path,
) -> std::io::Result<()> {
    if is_resume_output_complete(final_output) {
        let _ = std::fs::remove_file(partial_output);
        return Ok(());
    }
    if let Some(parent) = final_output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::File::open(partial_output)?.sync_all()?;
    std::fs::rename(partial_output, final_output)?;
    sync_parent_directory(final_output)
}

fn stable_resume_token(job_key: &str) -> String {
    let mut token = job_key
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .take(48)
        .collect::<String>();
    if token.is_empty() {
        token.push_str("job");
    }
    token
}

fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(unix)]
#[must_use]
pub(crate) fn available_disk_space(path: &Path) -> Option<u64> {
    use std::ffi::CString;

    let c_path = CString::new(path.to_string_lossy().as_bytes()).ok()?;
    // SAFETY: `stat` is immediately initialized by `statvfs` on success.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is NUL-terminated and `stat` points to valid writable memory.
    let ret = unsafe { libc::statvfs(c_path.as_ptr(), &raw mut stat) };
    if ret == 0 {
        Some(stat.f_bavail * stat.f_frsize)
    } else {
        None
    }
}

#[cfg(not(unix))]
#[must_use]
pub(crate) fn available_disk_space(_path: &Path) -> Option<u64> {
    None
}

// SW eq/transpose/etc. can't run on hwsurfaces; skip them when input is HEVC 10-bit and target is H.264.
#[must_use]
pub(crate) fn detect_hevc_10bit_to_h264(input: &Path, target_codec: &str) -> bool {
    if target_codec != "h264" {
        return false;
    }
    let output = BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "quiet",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=pix_fmt,codec_name",
            "-of",
            "csv=p=0",
            // `--` defends against filenames starting with `-`.
            "--",
        ])
        .arg(input)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(10))
        .build()
        .run();
    let Ok(out) = output else { return false };
    let text = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = text.trim().splitn(2, ',').collect();
    if parts.len() < 2 {
        return false;
    }
    let pix_fmt = parts[0];
    let codec = parts[1];
    let is_10bit = pix_fmt.contains("p10") || pix_fmt.contains("10le");
    let is_hevc = codec == "hevc" || codec == "h265";
    if is_10bit && is_hevc {
        log::info!(
            "HEVC 10-bit detected for {} — skipping video filters for GPU optimization",
            input.display()
        );
        true
    } else {
        false
    }
}
