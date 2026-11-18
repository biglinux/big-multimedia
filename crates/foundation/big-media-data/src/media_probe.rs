//! ffprobe's JSON report on a media file.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessSpec};

use crate::error::{Error, Result};

/// ffprobe's format and stream report for `path`, stopped after 30 seconds
/// or once `cancelled` is set. Blocking: call it from a worker thread.
///
/// # Errors
/// [`Error::Cancelled`] when `cancelled` stops the probe;
/// [`Error::InvalidArgument`] when ffprobe cannot run, times out, fails, or
/// prints no JSON.
pub fn probe_json(path: &str, cancelled: &AtomicBool) -> Result<serde_json::Value> {
    let result = BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
            // A file name starting with `-` must not read as a flag.
            "--",
            path,
        ])
        .timeout(Duration::from_secs(30))
        .build()
        .run_cancellable(cancelled)
        .map_err(|e| match e {
            BigSubprocessError::Cancelled => Error::Cancelled,
            _ => Error::invalid_argument("media_file", "Could not complete the media probe"),
        })?;
    if !result.status.success() {
        return Err(Error::invalid_argument("media_file", "Media probe failed"));
    }
    serde_json::from_slice(&result.stdout)
        .map_err(|_| Error::invalid_argument("media_file", "Invalid media probe response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn missing_file_is_refused() {
        let probe = probe_json("/nonexistent/song.flac", &AtomicBool::new(false));
        assert!(matches!(probe, Err(Error::InvalidArgument { .. })));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn set_flag_cancels_probe() {
        let probe = probe_json("/nonexistent/song.flac", &AtomicBool::new(true));
        assert!(matches!(probe, Err(Error::Cancelled)));
    }
}
