//! Boot smoke for big-video-converter. Mirror of the audio variant —
//! see big-audio-converter/tests/smoke.rs for the full rationale on
//! why this is construction-only (RelmApp::run blocks the GTK main
//! loop and mutates process-global bootstrap state).
//!
//! The `dlopen` module contract is tested in the suite-module workspace
//! `big-modules/big-converter-video`, not here — this crate ships only
//! the standalone app binding.

use big_converter_ui::{ConverterMode, RunOptions};
use big_video_converter::APP_MODE;

#[test]
fn run_options_video_constructs() {
    let opts = RunOptions {
        mode: ConverterMode::Video,
        log_filter: "big_media_runtime=info,big_video_converter=info",
        bootstrap_env: None,
        include_audio_resources: false,
    };

    assert!(matches!(opts.mode, ConverterMode::Video));
    assert!(!opts.include_audio_resources);
    assert!(opts.bootstrap_env.is_none());
    assert!(opts.log_filter.contains("big_video_converter"));
}

#[test]
fn exported_app_mode_is_video() {
    assert!(matches!(APP_MODE, ConverterMode::Video));
}
