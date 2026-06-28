//! `big-audio-player` library crate.
//!
//! The audio player UI built on top of the shared `big-media-runtime`
//! modules. The thin [`main`](../big_audio_player/index.html) binary calls
//! [`run`]. Exposing the UI as a library (mirroring `big-converter-ui`) lets
//! the Relm4 components compile as public API during the in-progress port,
//! instead of tripping `dead_code` while built alongside the legacy path.

pub use big_media_runtime::{
    cast, config, db, i18n, library, media, mpris, player, recent, settings, shared,
};

pub mod audio;
mod resources;

static EMBED_BOOT: std::sync::Once = std::sync::Once::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AudioPlayerWindowSpec {
    title: &'static str,
    default_width: i32,
    default_height: i32,
    minimum_width: i32,
    minimum_height: i32,
}

fn audio_player_window_spec() -> AudioPlayerWindowSpec {
    AudioPlayerWindowSpec {
        title: "Big Audio Player",
        default_width: 1200,
        default_height: 720,
        minimum_width: 640,
        minimum_height: 400,
    }
}

/// Wrap the player content (the shell's `ToastOverlay` Root, which is window-
/// agnostic — RESTRUCTURE ADR D14) in an application window with the standard
/// player geometry + title. Each mount point supplies its own window.
fn build_player_window(
    app: &impl gtk::prelude::IsA<gtk::Application>,
    content: &adw::ToastOverlay,
) -> adw::ApplicationWindow {
    use gtk::prelude::*;
    let spec = audio_player_window_spec();
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(spec.default_width)
        .default_height(spec.default_height)
        .title(i18n::i18n(spec.title))
        .content(content)
        .build();
    window.set_size_request(spec.minimum_width, spec.minimum_height);
    window
}

/// Owner handle for an embedded audio player: the window hosting the player
/// content + the shell controller (the keepalive). The host keeps it alive for
/// the module's lifetime; dropping the controller shuts the component tree down.
pub struct AudioEmbed {
    /// The window mounting the player content (for the host's module handle).
    // bigagents: app-local-window-owner — this handle IS the owner the host
    // keeps alive for the module's lifetime; a weak ref would drop the window.
    pub window: gtk::Window,
    /// The shell controller — drives the component; dropping it tears it down.
    pub controller: relm4::Controller<audio::ui::audio_shell::AudioShell>,
}

/// Open the audio-player shell as a window of the host's running
/// application (multicall hosts: big-host). Mirrors
/// `big_converter_ui::open_in_application`: performs the app-specific boot
/// exactly once (gettext — shared media domain —, numeric locale, component
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
        shared::app_base::init_gettext();
        shared::app_base::init_numeric_locale();
        big_media_components::install_media_components_translator(big_media_runtime::i18n::i18n);
        big_player_ui::favicon_cache::set_cache_namespace("big-audio-player");
        shared::app_base::register_resource_bytes(big_media_runtime::resources::AUDIO);
        shared::app_base::register_resource_bytes(resources::AUDIO_PLAYER_SHELL);
        audio::app::install_audio_accels(&relm4::main_application());
    });
}

/// Mount the audio player as a `PaneContent` — the unified-shell media pane
/// (RESTRUCTURE K2-T5, the first concrete media pane). The shell Root is window-
/// agnostic content (ADR D14), so this wraps it in a `ControllerPane` instead of
/// a window: the returned pane carries the shell controller as its keepalive
/// (dropping the pane shuts the shell down) and reports `OnDemand` keyboard need.
/// A container (`BigPaneTabs`, the background host, …) mounts it like any pane.
#[must_use]
pub fn mount_as_pane() -> big_relm4_components::pane::ControllerPane {
    use big_relm4_components::pane::{ControllerPane, KeyboardNeed};
    use relm4::{Component, ComponentController};

    embed_boot();
    let controller = audio::ui::audio_shell::AudioShell::builder()
        .launch(())
        .detach();
    // Clone the root handle so `controller` can move into the pane as the keepalive.
    let root = controller.widget().clone();
    ControllerPane::new(
        &root,
        i18n::i18n("Big Audio Player"),
        KeyboardNeed::OnDemand,
        controller,
    )
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

/// Deliver file paths to the running shell — the same thread-local broker
/// `Application::open` uses in standalone mode. No-op when `paths` is empty.
/// Multicall hosts call this right after [`open_in_application`] (or on
/// re-invocation with files while the module is already open).
pub fn open_paths(paths: Vec<String>) {
    if let Some(message) = open_paths_message_for_shell(paths) {
        audio::ui::audio_shell::send_to_shell(message);
    }
}

fn open_paths_message_for_shell(
    paths: Vec<String>,
) -> Option<audio::ui::audio_shell::AudioShellInput> {
    if paths.is_empty() {
        None
    } else {
        Some(audio::ui::audio_shell::AudioShellInput::OpenPaths(paths))
    }
}

/// Initialize logging, gettext, locale, and GResources, then run the GTK
/// application. Returns the process exit code.
#[must_use]
pub fn run() -> gtk::glib::ExitCode {
    // Cap malloc arenas before any threads spawn — curbs per-thread arena
    // retention from media worker threads (feed/metadata/image/mpv).
    shared::app_base::tune_allocator();
    shared::app_base::init_logging(
        "big_media_runtime=info,big_audio_player=info,lofty=error,glycin=error",
    );
    shared::app_base::init_gettext();
    shared::app_base::init_numeric_locale();
    big_media_components::install_media_components_translator(big_media_runtime::i18n::i18n);
    // Name the shared radio/favicon disk cache after this app (generic in the
    // framework; the app supplies its own id).
    big_player_ui::favicon_cache::set_cache_namespace("big-audio-player");
    shared::app_base::register_resource_bytes(big_media_runtime::resources::AUDIO);
    shared::app_base::register_resource_bytes(resources::AUDIO_PLAYER_SHELL);

    // The `AudioShell` Root is now the mountable content (a ToastOverlay), not a
    // window (RESTRUCTURE ADR D14), so we boot manually instead of `RelmApp::run`
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_player_window_spec_matches_shell_geometry() {
        let spec = audio_player_window_spec();

        assert_eq!(spec.title, "Big Audio Player");
        assert_eq!(spec.default_width, 1200);
        assert_eq!(spec.default_height, 720);
        assert_eq!(spec.minimum_width, 640);
        assert_eq!(spec.minimum_height, 400);
    }

    #[test]
    fn empty_open_paths_do_not_emit_shell_message() {
        assert!(open_paths_message_for_shell(Vec::new()).is_none());
    }

    #[test]
    fn non_empty_open_paths_preserve_order_for_shell() {
        let paths = vec!["first.ogg".to_string(), "second.flac".to_string()];

        match open_paths_message_for_shell(paths) {
            Some(audio::ui::audio_shell::AudioShellInput::OpenPaths(paths)) => {
                assert_eq!(paths, ["first.ogg", "second.flac"]);
            }
            other => panic!("expected OpenPaths shell message, got {other:?}"),
        }
    }
}
