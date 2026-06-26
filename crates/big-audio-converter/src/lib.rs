// SPDX-License-Identifier: GPL-3.0-or-later
//! Thin lib so integration tests can link against this crate.
//!
//! Binary stays in `src/main.rs`. This file exposes a minimal API
//! that lets `tests/*.rs` reach the shared converter UI crate plus
//! the runtime without re-declaring the dep graph.

pub use big_converter_ui::{self, ConverterMode, RunOptions};

/// Boot the audio-converter UI. Equivalent to what `main.rs` does.
///
/// Returns the process exit code. Not called from tests — present only
/// so the binary can delegate.
pub fn main_impl() -> std::process::ExitCode {
    big_converter_ui::run(RunOptions {
        mode: ConverterMode::Audio,
        log_filter: "big_media_runtime=info,big_audio_converter=info",
        bootstrap_env: Some(("BIG_MEDIA_AUDIO_CONVERTER", "1")),
        include_audio_resources: true,
    })
}

/// Mode this binary boots in — `ConverterMode::Audio`.
pub const APP_MODE: ConverterMode = ConverterMode::Audio;

/// Env-var key the audio binding flips before GTK starts.
pub const BOOTSTRAP_ENV_KEY: &str = "BIG_MEDIA_AUDIO_CONVERTER";
/// Value paired with [`BOOTSTRAP_ENV_KEY`].
pub const BOOTSTRAP_ENV_VAL: &str = "1";
