//! Launch external converter (big-audio-converter, fallback to big-video-converter
//! with audio-mode env hint) for a list of media paths.

use std::path::Path;

use big_app_kit::subprocess::BigSubprocessSpec;

pub(super) fn is_available() -> bool {
    Path::new("/usr/bin/big-audio-converter").exists()
        || Path::new("/usr/sbin/big-audio-converter").exists()
        || Path::new("/usr/bin/big-video-converter").exists()
        || Path::new("/usr/sbin/big-video-converter").exists()
}

/// Falls back to `big-video-converter` with `BIG_MEDIA_AUDIO_CONVERTER=1`.
///
/// SECURITY: every user-supplied path is placed after a `--` end-of-options
/// marker so file names beginning with `-` cannot be reinterpreted as
/// command-line flags by the converter binary.
pub(super) fn launch(paths: &[String]) {
    let primary = BigSubprocessSpec::builder()
        .program("big-audio-converter")
        .arg("--")
        .args(paths)
        .build()
        .spawn_detached();
    if let Err(primary_err) = primary {
        let fallback = BigSubprocessSpec::builder()
            .program("big-video-converter")
            .env("BIG_MEDIA_AUDIO_CONVERTER", "1")
            .arg("--")
            .args(paths)
            .build()
            .spawn_detached();
        if let Err(fallback_err) = fallback {
            log::warn!(
                "failed to launch audio converter (primary: {primary_err}; fallback: {fallback_err})"
            );
        }
    }
}
