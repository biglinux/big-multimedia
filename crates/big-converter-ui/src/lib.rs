//! Shared UI tree for the BigLinux audio + video converters.
//!
//! Each app binary builds a thin wrapper that calls [`run`] with a
//! [`ConverterMode`] and the resource set it needs registered.
//!
//! # Architecture
//!
//! - [`run`] is the single boot entrypoint. It initializes logging,
//!   gettext, locale, registers GResource bundles, and spins up the
//!   shared [`Application`] / [`ConverterModel`] tree under `relm4`.
//! - [`ConverterMode`] selects between the audio and video bindings at
//!   runtime. It is pinned once at startup, then read globally through
//!   `converter::app::converter_mode`.
//! - All long-lived state (queue, sidebar adjustments, edit-page
//!   sessions, GSettings) lives inside the Relm4 model tree rooted at
//!   [`ConverterModel`]. The two app binaries (`big-audio-converter`
//!   and `big-video-converter`) keep no UI state of their own.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

// Internal re-exports for `crate::*` paths inside this crate. Not part of
// the public API — downstream consumers depend on `big_media_runtime`
// directly if they need its modules.
#[allow(unused_imports)]
pub(crate) use big_media_runtime::{
    cast, config, converter as runtime_converter, i18n, media, player, resources, settings, shared,
};

pub mod converter;
pub mod encode_options;
pub mod export;
mod host_launcher;

pub use converter::app::{Application, ConverterMode};
pub use converter::ui::shell::ConverterShell as ConverterModel;
pub use host_launcher::{launch_host_module, launch_host_module_code};

/// Legacy alias for [`ConverterModel`]; kept so older call sites keep compiling.
pub type AudioConverterModel = ConverterModel;

/// Boot-time options for the shared converter UI.
///
/// `RunOptions` is the single input to [`run`]. Each binary
/// (`big-audio-converter`, `big-video-converter`) constructs one of
/// these and hands it to `run`; nothing about the shared UI is
/// configurable after boot.
///
/// # Examples
///
/// Boot the audio binding with the audio GResource bundle registered
/// and a feature-gate env var set before GTK initializes:
///
/// ```no_run
/// use big_converter_ui::{ConverterMode, RunOptions, run};
///
/// let exit = run(RunOptions {
///     mode: ConverterMode::Audio,
///     log_filter: "info,big_media_runtime=debug",
///     bootstrap_env: Some(("BIG_MEDIA_AUDIO_CONVERTER", "1")),
///     include_audio_resources: true,
/// });
/// let _ = exit;
/// ```
pub struct RunOptions {
    /// Audio or video binding (selects sidebar layout, default formats, etc.).
    pub mode: ConverterMode,
    /// `env_logger` filter passed to [`big_media_runtime::shared::app_base::init_logging`].
    pub log_filter: &'static str,
    /// Optional `(key, value)` env-var pair set before GTK starts; used by
    /// the audio converter to flip a runtime feature gate.
    pub bootstrap_env: Option<(&'static str, &'static str)>,
    /// Whether to additionally register the audio `gresource` bundle.
    pub include_audio_resources: bool,
}

/// Boot-time options for embedding the converter shell in a host application.
///
/// Unlike [`RunOptions`], embedding does not own the process: logging and
/// allocator tuning are the host's job, and no `gtk::Application` is created —
/// the shell window attaches to `relm4::main_application()` (set by the host
/// via `RelmApp::from_app`).
pub struct EmbedOptions {
    /// Audio or video binding (selects sidebar layout, default formats, etc.).
    pub mode: ConverterMode,
    /// Whether to additionally register the audio `gresource` bundle.
    pub include_audio_resources: bool,
}

static EMBED_BASE_BOOT: std::sync::Once = std::sync::Once::new();
static EMBED_AUDIO_RESOURCES: std::sync::Once = std::sync::Once::new();

/// Open the converter shell as a window of the host's running application.
///
/// Embedded counterpart of [`run`] for multicall hosts (big-host): performs
/// the converter-specific boot (gettext, numeric locale, component
/// translator, GResource bundles — all idempotent), pins the
/// [`ConverterMode`], launches a detached [`ConverterModel`] and presents its
/// window. The host keeps the returned controller alive for as long as the
/// module is open; dropping it shuts the component down.
///
/// The caller owns process-wide concerns ([`run`] steps this skips):
/// logging, allocator tuning, and the `gtk::Application` itself — which must
/// already be registered as `relm4::main_application()`.
///
/// # Examples
///
/// ```no_run
/// use big_converter_ui::{ConverterMode, EmbedOptions, open_in_application};
///
/// let audio = open_in_application(EmbedOptions {
///     mode: ConverterMode::Audio,
///     include_audio_resources: true,
/// });
/// // The mode is per instance — the video binding can run alongside.
/// let video = open_in_application(EmbedOptions {
///     mode: ConverterMode::Video,
///     include_audio_resources: false,
/// });
/// # drop((audio, video));
/// ```
pub fn open_in_application(options: EmbedOptions) -> relm4::Controller<ConverterModel> {
    use gtk::prelude::GtkWindowExt;
    use relm4::{Component, ComponentController};

    EMBED_BASE_BOOT.call_once(|| {
        shared::app_base::init_gettext();
        shared::app_base::init_numeric_locale();
        big_media_components::install_media_components_translator(big_media_runtime::i18n::i18n);
        shared::app_base::register_resource_bytes(big_media_runtime::resources::VIDEO);
    });
    if options.include_audio_resources {
        EMBED_AUDIO_RESOURCES.call_once(|| {
            shared::app_base::register_resource_bytes(big_media_runtime::resources::AUDIO);
        });
    }
    let controller = ConverterModel::builder().launch(options.mode).detach();
    controller.widget().present();
    controller
}

/// Build and run the converter UI with the given options.
///
/// Performs the full boot sequence:
///
/// 1. Initialize `env_logger` with the provided filter.
/// 2. Bind the `gettext` catalogue and numeric locale.
/// 3. Apply [`RunOptions::bootstrap_env`] if set (so any downstream
///    feature gate that reads the env var sees it before GTK runs).
/// 4. Register the video GResource bundle (always) and the audio
///    bundle (when [`RunOptions::include_audio_resources`] is `true`).
/// 5. Construct an [`Application`] bound to the selected
///    [`ConverterMode`] and run it under `relm4` as a
///    [`ConverterModel`].
///
/// Blocks until the GTK main loop exits. Returns the process exit code.
///
/// # Examples
///
/// ```no_run
/// use big_converter_ui::{ConverterMode, RunOptions, run};
///
/// let exit = run(RunOptions {
///     mode: ConverterMode::Video,
///     log_filter: "info",
///     bootstrap_env: None,
///     include_audio_resources: false,
/// });
/// let _ = exit;
/// ```
pub fn run(options: RunOptions) -> std::process::ExitCode {
    // Cap malloc arenas before any threads spawn — curbs per-thread arena
    // retention from media worker threads (probe / transcode / image decode).
    shared::app_base::tune_allocator();
    shared::app_base::init_logging(options.log_filter);
    shared::app_base::init_gettext();
    if let Some((k, v)) = options.bootstrap_env {
        shared::app_base::set_bootstrap_env(k, v);
    }
    shared::app_base::init_numeric_locale();
    big_media_components::install_media_components_translator(big_media_runtime::i18n::i18n);
    shared::app_base::register_resource_bytes(big_media_runtime::resources::VIDEO);
    if options.include_audio_resources {
        shared::app_base::register_resource_bytes(big_media_runtime::resources::AUDIO);
    }
    let app = Application::new_with_mode(options.mode);
    relm4::RelmApp::from_app(app).run::<converter::ui::shell::ConverterShell>(options.mode);
    std::process::ExitCode::SUCCESS
}
