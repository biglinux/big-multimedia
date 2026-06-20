//! Boot smoke for big-audio-converter.
//!
//! `RunOptions` does not expose a `build_for_test()` / headless boot
//! path today: shared bootstrap (`shared::app_base::init_logging`,
//! `init_gettext`, `set_bootstrap_env`, `init_numeric_locale`,
//! `register_resource_bytes`) mutates process-global state, and the
//! eventual `RelmApp::run::<ConverterModel>` blocks the GTK main loop.
//!
//! So this smoke verifies:
//!   1. The crate's exported binding constants are coherent.
//!   2. A `RunOptions` instance equivalent to the one `main.rs` hands to
//!      `big_converter_ui::run` constructs without panic — i.e. the
//!      public types from the shared UI crate are reachable and the
//!      shape we depend on still compiles.
//!
//! Gap follow-up (see also report): a real headless boot needs a
//! `RunOptions::build_for_test()` (or similar) in `big-converter-ui`
//! that runs the bootstrap + builds the top window without calling
//! `RelmApp::run`. Until then, full UI smoke lives behind
//! `linux-ui-a11y` `kwin_wayland --virtual` harness, not `cargo test`.

use big_audio_converter::{APP_MODE, BOOTSTRAP_ENV_KEY, BOOTSTRAP_ENV_VAL};
use big_converter_ui::{ConverterMode, RunOptions};

#[test]
fn run_options_audio_constructs() {
    // Mirror the exact options main.rs builds. Pure construction —
    // never call `run()`; that would block + mutate process state.
    let opts = RunOptions {
        mode: ConverterMode::Audio,
        log_filter: "big_media_runtime=info,big_audio_converter=info",
        bootstrap_env: Some((BOOTSTRAP_ENV_KEY, BOOTSTRAP_ENV_VAL)),
        include_audio_resources: true,
    };

    assert!(matches!(opts.mode, ConverterMode::Audio));
    assert!(opts.mode.is_audio());
    assert!(opts.include_audio_resources);
    let (k, v) = opts.bootstrap_env.expect("audio binding sets env pair");
    assert_eq!(k, "BIG_MEDIA_AUDIO_CONVERTER");
    assert_eq!(v, "1");
    assert!(opts.log_filter.contains("big_audio_converter"));
}

#[test]
fn exported_constants_match_run_options() {
    // Binary's exported binding constants are the contract integration
    // tests (and packaging tooling) read; keep them aligned with the
    // RunOptions main.rs builds.
    assert!(matches!(APP_MODE, ConverterMode::Audio));
    assert_eq!(BOOTSTRAP_ENV_KEY, "BIG_MEDIA_AUDIO_CONVERTER");
    assert_eq!(BOOTSTRAP_ENV_VAL, "1");
}
