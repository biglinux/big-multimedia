//! `big-video-player` library crate.
//!
//! The video player UI built on the shared `big-media-runtime` modules. The
//! thin binary (`src/main.rs`) calls [`run`]. Exposing the UI as a library
//! (mirroring `big-audio-player`) lets the Relm4 components compile as public
//! API during the in-progress port instead of tripping `dead_code` while built
//! alongside the legacy god-object path.

pub use big_media_runtime::{
    cast, config, i18n, library, media, mpris, player, recent, settings, shared,
};

mod resources;
pub mod video;

static EMBED_BOOT: std::sync::Once = std::sync::Once::new();

/// Wrap the player content (the shell's window-agnostic `ToastOverlay` Root —
/// RESTRUCTURE ADR D14) in an application window with the standard geometry.
fn build_player_window(
    app: &impl gtk::prelude::IsA<gtk::Application>,
    content: &adw::ToastOverlay,
) -> adw::ApplicationWindow {
    use gtk::prelude::*;
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(1100)
        .default_height(680)
        .title(i18n::i18n("Big Video Player"))
        .content(content)
        .build();
    window.set_size_request(640, 400);
    window
}

/// App-specific boot done exactly once for the embed/pane paths.
fn embed_boot() {
    EMBED_BOOT.call_once(|| {
        shared::app_base::init_gettext();
        // Route shared big-media-components strings through the same gettext
        // domain init_gettext just bound (else they render untranslated here).
        big_media_components::install_media_components_translator(big_media_runtime::i18n::i18n);
        // Read lazily at backend install (realize) — safe to default here.
        shared::app_base::set_default_env("BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD", "1");
        shared::app_base::init_numeric_locale();
        shared::app_base::register_resource_bytes(big_media_runtime::resources::VIDEO);
        shared::app_base::register_resource_bytes(resources::VIDEO_PLAYER_SHELL);
    });
}

/// Owner handle for an embedded video player: the window hosting the content +
/// the shell controller (the keepalive). Dropping the controller tears the
/// component tree down.
pub struct VideoEmbed {
    /// The window mounting the player content (for the host's module handle).
    // bigagents: app-local-window-owner — a transient transfer handle, not a
    // signal-captured field: the host immediately moves this into its
    // `OpenModuleHandle.window` (the real owner) and drops the `VideoEmbed`.
    pub window: gtk::Window,
    /// The shell controller — drives the component; dropping it tears it down.
    pub controller: relm4::Controller<video::ui::video_shell::VideoShell>,
}

/// Mount the video player as a `PaneContent` — a unified-shell media pane
/// (RESTRUCTURE K2-T5). The shell Root is window-agnostic content (ADR D14), so
/// this wraps it in a `ControllerPane` carrying the controller as keepalive.
/// (mpv/GL realize when the content lands in a window; real-hardware GL
/// validation of playback is still required — see the doc's B4 GL gate.)
#[must_use]
pub fn mount_as_pane() -> big_relm4_components::pane::ControllerPane {
    use big_relm4_components::pane::{ControllerPane, KeyboardNeed};
    use relm4::{Component, ComponentController};

    embed_boot();
    let controller = video::ui::video_shell::VideoShell::builder()
        .launch(video::ui::video_shell::VideoShellMode::Player)
        .detach();
    let root = controller.widget().clone();
    let sender = controller.sender().clone();
    ControllerPane::new(
        &root,
        i18n::i18n("Big Video Player"),
        KeyboardNeed::OnDemand,
        controller,
    )
    .with_activity(move |activity| {
        let _ = sender.send(video::ui::video_shell::VideoShellInput::SetPaneActivity(
            activity,
        ));
    })
}

/// Mount only the mpv-backed video surface as desktop-background content.
/// This keeps the optimized video render path from the full player but omits
/// its header, transport controls, welcome screen, sidebar, and window gestures.
#[must_use]
pub fn mount_as_background_pane() -> big_relm4_components::pane::ControllerPane {
    use big_relm4_components::pane::{ControllerPane, KeyboardNeed};
    use relm4::{Component, ComponentController};

    embed_boot();
    let controller = video::ui::video_shell::VideoShell::builder()
        .launch(video::ui::video_shell::VideoShellMode::Background)
        .detach();
    let root = controller.widget().clone();
    let sender = controller.sender().clone();
    ControllerPane::new(
        &root,
        i18n::i18n("Video Background"),
        KeyboardNeed::OnDemand,
        controller,
    )
    .with_activity(move |activity| {
        let _ = sender.send(video::ui::video_shell::VideoShellInput::SetPaneActivity(
            activity,
        ));
    })
}

/// Open the video-player shell as a window of the host's running
/// application (multicall hosts: big-host). Performs the app-specific boot
/// exactly once (gettext — shared media domain —, numeric locale, GResource
/// bundles, the graphics-offload default) and launches a detached
/// [`video::ui::video_shell::VideoShell`]. The host keeps the returned
/// controller alive for as long as the module is open; dropping it shuts
/// the component tree down. The caller owns process-wide concerns ([`run`]
/// steps this skips): logging, allocator tuning, the GSK renderer
/// choice (the host's renderer is already decided), and the
/// `gtk::Application` itself.
pub fn open_in_application() -> VideoEmbed {
    use gtk::prelude::*;
    use relm4::{Component, ComponentController};

    embed_boot();
    let controller = video::ui::video_shell::VideoShell::builder()
        .launch(video::ui::video_shell::VideoShellMode::Player)
        .detach();
    // The host's application is the registered default (it boots via RelmApp).
    let window = build_player_window(&relm4::main_application(), controller.widget());
    window.present();
    VideoEmbed {
        window: window.upcast(),
        controller,
    }
}

/// Deliver file paths to the running shell — the same thread-local broker
/// `Application::open` uses in standalone mode. No-op when `paths` is empty.
/// Multicall hosts call this right after [`open_in_application`] (or on
/// re-invocation with files while the module is already open).
pub fn open_paths(paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    video::ui::video_shell::send_to_shell(video::ui::video_shell::VideoShellInput::OpenFiles(
        paths,
    ));
}

/// Best-effort detection of the NVIDIA proprietary driver, before GTK init: the
/// kernel module exposes these paths only when loaded (nouveau does not).
fn nvidia_proprietary_driver_in_use() -> bool {
    std::path::Path::new("/proc/driver/nvidia/version").exists()
        || std::path::Path::new("/dev/nvidia0").exists()
}

/// Initialize logging, gettext, locale, GSK renderer, and GResources,
/// then run the GTK application. Returns the process exit code.
#[must_use]
pub fn run() -> gtk::glib::ExitCode {
    // Stays on glibc malloc: its mmap threshold returns the large, short-lived
    // decode buffers to the OS on free. jemalloc measured 106 MB worse here.
    shared::app_base::tune_allocator();
    // Cap malloc arenas before any threads spawn — curbs per-thread arena
    // retention from media worker threads (decode / probe / image). Large frame
    shared::app_base::init_logging(
        "big_media_runtime=info,big_video_player=info,lofty=error,glycin=error",
    );
    shared::app_base::init_gettext();
    // Route shared big-media-components strings through the same gettext domain
    // init_gettext just bound (else they render untranslated in the video app).
    big_media_components::install_media_components_translator(big_media_runtime::i18n::i18n);
    // The video player renders libmpv into a GtkGLArea. Force (override, since the
    // session often exports GSK_RENDERER=cairo) the renderer that is stable for
    // that: "ngl" everywhere except NVIDIA proprietary, which needs the legacy
    // "gl" (the modern renderer retains GPU memory on resize there). Both "cairo"
    // and legacy "gl" leak one dmabuf fd per frame on Mesa software (llvmpipe) on
    // Wayland and crash playback after ~40 s. Mirrors big-host's
    // configure_process_environment.
    // SAFETY: main path, before GTK init and before app threads.
    let gsk_renderer = if nvidia_proprietary_driver_in_use() {
        "gl"
    } else {
        "ngl"
    };
    unsafe {
        std::env::set_var("GSK_RENDERER", gsk_renderer);
    }
    // Default to the GLArea video path. GraphicsOffload zero-copy is only a win
    // when the GPU exports DMA-BUF-importable (linear) modifiers; GPUs that
    // export non-linear modifiers (e.g. NVIDIA proprietary) fall back to a
    // cross-context GL-texture upload per frame that saturates the main thread
    // (sluggish UI, choppy video), whereas GLArea renders straight into GTK's GL
    // surface. The shared backend still rejects NVIDIA renderers even when this
    // variable is unset/false because NVIDIA's GdkGLTexture/GraphicsOffload
    // path grows GPU memory while resizing.
    shared::app_base::set_default_env("BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD", "1");
    shared::app_base::init_numeric_locale();
    shared::app_base::register_resource_bytes(big_media_runtime::resources::VIDEO);
    shared::app_base::register_resource_bytes(resources::VIDEO_PLAYER_SHELL);

    // The `VideoShell` Root is now mountable content (a ToastOverlay), not a
    // window (ADR D14), so we boot manually instead of `RelmApp::run` (which
    // requires a window Root): on activate, launch the shell and wrap its content
    // in the app window. The app subclass still owns the app-level actions,
    // accels, and the file-open broker.
    use gtk::prelude::*;
    use relm4::{Component, ComponentController};

    let app = video::app::Application::new();
    let shell: std::rc::Rc<
        std::cell::RefCell<Option<relm4::Controller<video::ui::video_shell::VideoShell>>>,
    > = std::rc::Rc::new(std::cell::RefCell::new(None));
    let shell_for_activate = shell.clone();
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let controller = video::ui::video_shell::VideoShell::builder()
            .launch(video::ui::video_shell::VideoShellMode::Player)
            .detach();
        let window = build_player_window(app, controller.widget());
        window.present();
        *shell_for_activate.borrow_mut() = Some(controller);
    });
    app.run()
}
