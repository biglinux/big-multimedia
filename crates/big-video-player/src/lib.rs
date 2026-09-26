//! `big-video-player` library crate.
//!
//! The video player UI built on the shared `big-media-runtime` modules. The
//! thin binary (`src/main.rs`) calls [`run`]; multicall hosts (big-host,
//! big-modules) embed the player through [`open_in_application`] and
//! [`open_paths`].

pub use big_media_runtime::{cast, config, i18n, media, player, recent, settings, shared};

mod resources;
pub mod video;

static EMBED_BOOT: std::sync::Once = std::sync::Once::new();

/// Wrap the player content (the shell's window-agnostic `gtk::Box` Root)
/// in an application window with the standard geometry.
fn build_player_window(
    app: &impl gtk::prelude::IsA<gtk::Application>,
    content: &gtk::Box,
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
        // Read lazily at backend install (realize) — safe to default here. See
        // `run` for why the offload path is not the default.
        shared::app_base::set_default_env("BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD", "1");
        shared::app_base::boot_player(&[
            big_media_runtime::resources::VIDEO,
            resources::VIDEO_PLAYER_SHELL,
        ]);
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
        .launch(())
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

/// Initialize logging, gettext, locale, GSK renderer, and GResources,
/// then run the GTK application. Returns the process exit code.
#[must_use]
pub fn run() -> gtk::glib::ExitCode {
    shared::app_base::init_logging(
        "big_media_runtime=info,big_video_player=info,lofty=error,glycin=error",
    );
    // mpv renders through GL, so the GSK renderer has to be the OpenGL one —
    // override it, since the session often exports GSK_RENDERER=cairo, which
    // would push every frame through a software upload. "opengl" is the name
    // GTK accepts without a warning: the old GL renderer was removed in GTK
    // 4.18 and "ngl" is no longer recognised at all (GTK ignores it and falls
    // back to its own default, which may be Vulkan).
    // SAFETY: main path, before GTK init and before app threads.
    unsafe {
        std::env::set_var("GSK_RENDERER", "opengl");
    }
    // Keep the GLArea video path as the default. Measured on 2026-09-17 on an
    // AMD Cezanne and an Intel HD 3000 laptop playing 1080p30: once the render
    // call stopped blocking (see `FastRenderContext::render`), offload and
    // GLArea cost the same CPU — 3.6 % against 3.5 % on the AMD, 21.3 % against
    // 21.9 % on the Intel — and offload holds an extra GL context and texture
    // pool while idle (+8 MiB on the AMD, +25 MiB on the Intel). It does give
    // real DMA-BUF zero-copy, so it stays one variable away for anyone whose
    // compositor benefits from the subsurface; it just buys nothing by default.
    shared::app_base::set_default_env("BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD", "1");
    shared::app_base::boot_player(&[
        big_media_runtime::resources::VIDEO,
        resources::VIDEO_PLAYER_SHELL,
    ]);

    // The `VideoShell` Root is mountable content (a `gtk::Box`), not a
    // window, so we boot manually instead of `RelmApp::run` (which
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
            .launch(())
            .detach();
        let window = build_player_window(app, controller.widget());
        shared::app_base::log_active_renderer_once(&window);
        window.present();
        *shell_for_activate.borrow_mut() = Some(controller);
    });
    app.run()
}
