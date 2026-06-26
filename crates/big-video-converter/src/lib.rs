// SPDX-License-Identifier: GPL-3.0-or-later
//! Thin lib so integration tests can link against this crate.
//!
//! Binary stays in `src/main.rs`. This file exposes a minimal API
//! that lets `tests/*.rs` reach the shared converter UI crate plus
//! the runtime without re-declaring the dep graph.
//!
//! The `dlopen`-able `BigModule` wrapper for `big-host` is NOT here —
//! it lives in the suite-module workspace `big-modules/big-converter-video`
//! (ships `libbig_converter_video.so`). This crate is the standalone app
//! binding only.

pub use big_converter_ui::{self, ConverterMode, RunOptions};

/// Boot the video-converter UI in-process. Mirrors the embedded module's
/// shell; the standalone binary (`src/main.rs`) instead execs `big-host`
/// so it shares the host-managed mpv engine + `big-convertd` queue.
///
/// Returns the process exit code.
pub fn run_video_converter_application() -> std::process::ExitCode {
    big_converter_ui::run(RunOptions {
        mode: ConverterMode::Video,
        log_filter: "big_media_runtime=info,big_video_converter=info",
        bootstrap_env: None,
        include_audio_resources: false,
    })
}

/// Mode this binding boots in — `ConverterMode::Video`.
pub const APP_MODE: ConverterMode = ConverterMode::Video;
