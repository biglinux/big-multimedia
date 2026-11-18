//! ffprobe's JSON report on a media file, and the protocols ffmpeg and
//! ffprobe may open for an input.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessSpec};

use crate::error::{Error, Result};

/// ffmpeg and ffprobe input options for a file on this computer: a browsed
/// file named `concat:…` or holding an HLS list would otherwise open other
/// protocols, the network among them. Goes before the input it guards.
pub const LOCAL_INPUT_PROTOCOLS: [&str; 2] = ["-protocol_whitelist", "file,pipe"];

/// [`LOCAL_INPUT_PROTOCOLS`] for a local `input`; a stream URL gets the
/// network protocols its streams use instead, never `file`.
#[must_use]
pub fn input_protocols(input: &str) -> [&'static str; 2] {
    if big_os_kit::url::is_stream_uri(input) {
        [
            "-protocol_whitelist",
            "http,https,tls,tcp,udp,rtp,rtmp,rtmps,mmsh,mmst,srt,crypto,data,hls,httpproxy",
        ]
    } else {
        LOCAL_INPUT_PROTOCOLS
    }
}

fn probe_spec(path: &str) -> BigSubprocessSpec {
    BigSubprocessSpec::builder()
        .program("ffprobe")
        .args(input_protocols(path))
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
}

/// ffprobe's format and stream report for `path`, stopped after 30 seconds
/// or once `cancelled` is set. Blocking: call it from a worker thread.
///
/// # Errors
/// [`Error::Cancelled`] when `cancelled` stops the probe;
/// [`Error::InvalidArgument`] when ffprobe cannot run, times out, fails, or
/// prints no JSON.
pub fn probe_json(path: &str, cancelled: &AtomicBool) -> Result<serde_json::Value> {
    let result = probe_spec(path)
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
    fn probe_inputs_keep_their_protocols() {
        for (input, protocols) in [
            ("/music/concat:a|b.flac", "file,pipe"),
            ("-name.flac", "file,pipe"),
            ("HTTPS://radio/live", input_protocols("https://h")[1]),
        ] {
            let argv = probe_spec(input).try_resolved().unwrap().argv;
            assert_eq!(&argv[..2], ["-protocol_whitelist", protocols], "{input}");
            assert_eq!(argv.last().map(String::as_str), Some(input));
        }
        assert!(!input_protocols("rtsp://cam/live")[1].contains("file"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn set_flag_cancels_probe() {
        let probe = probe_json("/nonexistent/song.flac", &AtomicBool::new(true));
        assert!(matches!(probe, Err(Error::Cancelled)));
    }
}
