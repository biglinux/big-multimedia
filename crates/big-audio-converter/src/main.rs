// SPDX-License-Identifier: GPL-3.0-or-later
//! Thin binary entrypoint for `big-audio-converter`; delegates to the crate
//! library's application runner.

fn main() -> std::process::ExitCode {
    big_audio_converter::run_audio_converter_application()
}
