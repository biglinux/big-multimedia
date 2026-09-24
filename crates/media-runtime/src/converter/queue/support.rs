//! Output naming, disk-space checks and background metadata completion.

use std::path::Path;

pub(crate) use big_media_engines::output_paths::{dedup_output_path, segment_output_path};

use super::ConversionQueueEntry;
use crate::converter::ffmpeg::progress;

/// Complete pending metadata on the conversion worker before using it.
/// An already-probed entry needs no additional subprocesses.
pub(crate) fn ensure_entry_probed(queue_entry: &mut ConversionQueueEntry) {
    if queue_entry.probed {
        return;
    }
    queue_entry.duration_secs = progress::probe_duration(&queue_entry.input);
    queue_entry.input_audio_channels = progress::probe_audio_channels(&queue_entry.input);
    queue_entry.probed = true;
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
