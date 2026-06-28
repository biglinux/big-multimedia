// SPDX-License-Identifier: GPL-3.0-or-later
//! Thin lib so integration tests can link against this crate.
//!
//! Binary stays in `src/main.rs`. This file exposes a minimal API
//! that lets `tests/*.rs` reach the shared converter UI crate plus
//! the runtime without re-declaring the dep graph.

pub use big_converter_ui::{self, ConverterMode, RunOptions};

/// Env logger filter used by the standalone audio converter binding.
const AUDIO_CONVERTER_LOG_FILTER: &str = "big_media_runtime=info,big_audio_converter=info";
/// Mode this binary boots in: [`ConverterMode::Audio`].
pub const APP_MODE: ConverterMode = ConverterMode::Audio;
/// Env-var key the audio binding flips before GTK starts.
pub const BOOTSTRAP_ENV_KEY: &str = "BIG_MEDIA_AUDIO_CONVERTER";
/// Value paired with [`BOOTSTRAP_ENV_KEY`].
pub const BOOTSTRAP_ENV_VAL: &str = "1";
/// The audio binding must register its extra audio resources before UI boot.
const INCLUDE_AUDIO_RESOURCES: bool = true;

/// Build the boot options used by the standalone audio converter.
#[must_use]
pub fn audio_converter_run_options() -> RunOptions {
    RunOptions {
        mode: APP_MODE,
        log_filter: AUDIO_CONVERTER_LOG_FILTER,
        bootstrap_env: Some((BOOTSTRAP_ENV_KEY, BOOTSTRAP_ENV_VAL)),
        include_audio_resources: INCLUDE_AUDIO_RESOURCES,
    }
}

fn run_audio_converter_application_with_runner(
    run_converter_ui: impl FnOnce(RunOptions) -> std::process::ExitCode,
) -> std::process::ExitCode {
    run_converter_ui(audio_converter_run_options())
}

/// Boot the audio-converter UI. Equivalent to what `main.rs` does.
///
/// Returns the process exit code. Not called from tests — present only
/// so the binary can delegate.
pub fn run_audio_converter_application() -> std::process::ExitCode {
    run_audio_converter_application_with_runner(big_converter_ui::run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_receives_standalone_audio_options() {
        let has_runner_been_called = std::cell::Cell::new(false);

        let exit_code = run_audio_converter_application_with_runner(|options| {
            has_runner_been_called.set(true);
            assert_eq!(options.mode, ConverterMode::Audio);
            assert_eq!(options.log_filter, AUDIO_CONVERTER_LOG_FILTER);
            assert_eq!(
                options.bootstrap_env,
                Some((BOOTSTRAP_ENV_KEY, BOOTSTRAP_ENV_VAL))
            );
            assert_eq!(options.include_audio_resources, INCLUDE_AUDIO_RESOURCES);
            std::process::ExitCode::SUCCESS
        });

        assert!(has_runner_been_called.get());
        assert_eq!(exit_code, std::process::ExitCode::SUCCESS);
    }
}
