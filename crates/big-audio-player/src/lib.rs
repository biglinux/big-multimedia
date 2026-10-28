//! `big-audio-player` library crate.
//!
//! The audio player UI built on top of the shared `big-media-runtime`
//! modules. The thin [`main`](../big_audio_player/index.html) binary calls
//! [`run`]; multicall hosts (big-host, big-modules) embed the player through
//! [`open_in_application`] and [`open_paths`].

pub use big_media_runtime::{
    cast, config, db, i18n, library, media, player, recent, settings, shared,
};

pub mod audio;
mod resources;

static EMBED_BOOT: std::sync::Once = std::sync::Once::new();

/// Wrap the player content (the shell's window-agnostic `gtk::Box` Root)
/// in an application window with the standard player geometry + title. Each mount point supplies its own window.
fn build_player_window(
    app: &impl gtk::prelude::IsA<gtk::Application>,
    content: &gtk::Box,
) -> adw::ApplicationWindow {
    use gtk::prelude::*;
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(1200)
        .default_height(720)
        .title(i18n::i18n("Big Audio Player"))
        .content(content)
        .build();
    window.set_size_request(640, 400);
    // Big Gnome Center frosts the windows that carry their app's class
    // (GNOME 51, "Windows" under Frosted glass). It shows through the
    // theme's window colors; custom colors and the album art paint over it.
    window.add_css_class("big-audio-player");
    // With the tray icon on, closing hides the window and playback goes on;
    // the icon brings it back, and its Quit (or Ctrl+Q) ends the player.
    // Connected here, before any other close handler, so it decides first.
    window.connect_close_request(|window| {
        if audio::ui::audio_shell::close_hides_window() {
            window.set_visible(false);
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    shared::app_base::ignore_repeated_pointer_motion(&window);
    window
}

/// Owner handle for an embedded audio player: the window hosting the player
/// content + the shell controller (the keepalive). The host keeps it alive for
/// the module's lifetime; dropping the controller shuts the component tree down.
pub struct AudioEmbed {
    /// The window mounting the player content (for the host's module handle).
    // this handle is the owner the host
    // keeps alive for the module's lifetime; a weak ref would drop the window.
    pub window: gtk::Window,
    /// The shell controller. Drives the component; dropping it tears it down.
    pub controller: relm4::Controller<audio::ui::audio_shell::AudioShell>,
}

/// Open the audio-player shell as a window of the host's running
/// application (multicall hosts: big-host). Performs the app-specific boot
/// exactly once (gettext with the shared media domain, numeric locale, component
/// translator, favicon cache namespace, GResource bundles) and launches a
/// detached [`audio::ui::audio_shell::AudioShell`]. The host keeps the
/// returned controller alive for as long as the module is open; dropping it
/// shuts the component tree down. The caller owns process-wide concerns
/// ([`run`] steps this skips): logging, allocator tuning, and the
/// `gtk::Application` itself.
/// App-specific boot done exactly once for the embed/pane paths (gettext shared
/// media domain, numeric locale, component translator, favicon namespace,
/// GResource bundles). The standalone [`run`] does these inline.
fn embed_boot() {
    EMBED_BOOT.call_once(|| {
        shared::app_base::boot_player(&[
            big_media_runtime::resources::AUDIO,
            resources::AUDIO_PLAYER_SHELL,
        ]);
        big_player_ui::favicon_cache::set_cache_namespace("big-audio-player");
        audio::app::install_audio_accels(&relm4::main_application());
    });
}

pub fn open_in_application() -> AudioEmbed {
    use gtk::prelude::*;
    use relm4::{Component, ComponentController};

    embed_boot();
    let controller = audio::ui::audio_shell::AudioShell::builder()
        .launch(())
        .detach();
    // The host's application is the registered default (it boots via RelmApp).
    let window = build_player_window(&relm4::main_application(), controller.widget());
    window.present();
    AudioEmbed {
        window: window.upcast(),
        controller,
    }
}

/// Deliver file paths to the running shell through the same thread-local broker
/// `Application::open` uses in standalone mode. No-op when `paths` is empty.
/// Multicall hosts call this right after [`open_in_application`] (or on
/// re-invocation with files while the module is already open).
pub fn open_paths(paths: Vec<String>) {
    if let Some(message) = audio::app::open_paths_message(paths) {
        audio::ui::audio_shell::send_to_shell(message);
    }
}

/// Initialize logging, gettext, locale, and GResources, then run the GTK
/// application. Returns the process exit code.
#[must_use]
pub fn run() -> gtk::glib::ExitCode {
    shared::app_base::init_logging(
        "big_media_runtime=info,big_audio_player=info,lofty=error,glycin=error",
    );
    // GL instead of GTK's default Vulkan: less memory at startup for the same
    // CPU while scrolling the library. A renderer the session exports still wins.
    shared::app_base::use_gl_renderer(false);
    shared::app_base::boot_player(&[
        big_media_runtime::resources::AUDIO,
        resources::AUDIO_PLAYER_SHELL,
    ]);
    // Name the shared radio/favicon disk cache after this app (generic in the
    // framework; the app supplies its own id).
    big_player_ui::favicon_cache::set_cache_namespace("big-audio-player");

    // The `AudioShell` Root is mountable content (a `gtk::Box`), not a
    // window, so we boot manually instead of `RelmApp::run`
    // (which requires a window Root): on activate, launch the shell and wrap its
    // content in the application window. The app subclass still owns the
    // app-level actions, accels, and the file-open broker.
    use gtk::prelude::*;
    use relm4::{Component, ComponentController};

    let app = audio::app::Application::new();
    let shell: std::rc::Rc<
        std::cell::RefCell<Option<relm4::Controller<audio::ui::audio_shell::AudioShell>>>,
    > = std::rc::Rc::new(std::cell::RefCell::new(None));
    let shell_for_activate = shell.clone();
    app.connect_activate(move |app| {
        // Single-instance: a second launch re-presents the existing window.
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let controller = audio::ui::audio_shell::AudioShell::builder()
            .launch(())
            .detach();
        let window = build_player_window(app, controller.widget());
        window.present();
        *shell_for_activate.borrow_mut() = Some(controller);
    });
    app.run()
}
