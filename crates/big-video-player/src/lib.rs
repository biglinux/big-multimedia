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
        // Read lazily at backend install (realize), so defaulting here is safe. See
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
    // a transient transfer handle, not a
    // signal-captured field: the host immediately moves this into its
    // `OpenModuleHandle.window` (the real owner) and drops the `VideoEmbed`.
    pub window: gtk::Window,
    /// The shell controller: drives the component; dropping it tears it down.
    pub controller: relm4::Controller<video::ui::video_shell::VideoShell>,
}

/// Open the video-player shell as a window of the host's running
/// application (multicall hosts: big-host). Performs the app-specific boot
/// exactly once (gettext with the shared media domain, numeric locale, GResource
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

/// Deliver file paths to the running shell through the thread-local broker
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
    // mpv renders through GL, so the GSK renderer has to be the OpenGL one.
    // Override it, since the session often exports GSK_RENDERER=cairo, which
    // would push every frame through a software upload. "opengl" is the name
    // GTK 4.18+ accepts without a warning: it ignores "ngl" and falls back to
    // its own default, which may be Vulkan.
    shared::app_base::use_gl_renderer(true);
    // Keep the GLArea video path as the default. With a non-blocking render
    // call (see `FastRenderContext::render`), offload and GLArea cost the same
    // CPU, and offload holds an extra GL context and texture pool while idle.
    // It does give
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
        shared::app_base::ignore_repeated_pointer_motion(&window);
        window.present();
        *shell_for_activate.borrow_mut() = Some(controller);
    });
    app.run()
}

#[cfg(test)]
mod lifetime_tests {
    use gtk::prelude::*;
    use relm4::ComponentController;
    use std::{cell::Cell, rc::Rc};

    #[test]
    #[ignore = "requires an isolated GTK session and home"]
    fn embedded_video_finalizes_after_close() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.biglinux.VideoLifetimeTest")
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let _relm = relm4::RelmApp::<()>::from_app(app);
        let context = gtk::glib::MainContext::default();
        let finalized = Rc::new(Cell::new(0));
        struct Finalized(Rc<Cell<usize>>);
        impl Drop for Finalized {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        for cycle in 1..=25 {
            let player = super::open_in_application();
            for widget in [
                player.window.upcast_ref::<gtk::Widget>(),
                player.controller.widget().upcast_ref(),
            ] {
                // SAFETY: this test owns the key and never reads its value.
                unsafe {
                    widget.set_qdata(
                        gtk::glib::Quark::from_str("video-finalized"),
                        Finalized(finalized.clone()),
                    );
                }
            }
            while context.pending() {
                context.iteration(false);
            }
            player.window.destroy();
            drop(player);
            for _ in 0..100 {
                while context.pending() {
                    context.iteration(false);
                }
                if finalized.get() == cycle * 2 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(
                finalized.get(),
                cycle * 2,
                "window and content must finalize"
            );
        }
    }
}
