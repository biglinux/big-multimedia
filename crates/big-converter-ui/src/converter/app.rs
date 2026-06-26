//! Converter application — `adw::Application` subclass shared by the video
//! and audio converter binaries.

use adw::prelude::*;
use adw::subclass::prelude::*;
use big_app_kit::desktop::{install_quit_action, install_static_accels};
use gtk::{gio, glib};
use std::cell::Cell;

/// D-Bus application id of the video-converter binding.
pub const APP_ID_VIDEO: &str = crate::config::video_converter::APP_ID;
/// Human-readable name of the video converter (window title, About dialog).
pub const APP_NAME_VIDEO: &str = crate::config::video_converter::APP_NAME;
/// D-Bus application id of the audio-converter binding.
pub const APP_ID_AUDIO: &str = crate::config::audio_converter::APP_ID;
/// Human-readable name of the audio converter (window title, About dialog).
pub const APP_NAME_AUDIO: &str = crate::config::audio_converter::APP_NAME;

/// Runtime selector between the audio and video binding of the shared UI.
///
/// Carried PER INSTANCE: the shell, sidebar and settings receive it
/// explicitly (no process global), so an embedding host can run the audio
/// and the video converter side by side in one process. Drives sidebar
/// layout, default container/codec combos, and which dialogs are reachable.
///
/// # Examples
///
/// ```
/// use big_converter_ui::ConverterMode;
///
/// let mode = ConverterMode::Audio;
/// assert!(mode.is_audio());
/// assert!(!ConverterMode::Video.is_audio());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConverterMode {
    /// Video-converter binding: full edit page with video panel and
    /// codec dialogs.
    Video,
    /// Audio-converter binding: audio-only sidebar, audio edit page,
    /// no video panel.
    Audio,
}

impl ConverterMode {
    /// Returns `true` when this mode is [`ConverterMode::Audio`].
    #[must_use]
    pub fn is_audio(self) -> bool {
        matches!(self, Self::Audio)
    }
}

mod imp {
    use super::*;

    pub struct Application {
        /// Binding this application instance runs (standalone binaries).
        /// Per-instance — there is deliberately NO process-global mode.
        pub(super) mode: Cell<ConverterMode>,
    }

    impl Default for Application {
        fn default() -> Self {
            Self {
                mode: Cell::new(ConverterMode::Video),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "BigConverterApplication";
        type Type = super::Application;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for Application {}

    impl ApplicationImpl for Application {
        // No `activate` override: the default emits the `activate` signal that
        // relm4's `RelmApp::run` connects to (it creates the window at startup
        // and shows it on activate). Overriding without chaining would suppress
        // that signal and the window would never appear.

        fn open(&self, files: &[gio::File], _hint: &str) {
            // Ensure the relm4-managed window is shown, then deliver the files
            // through this binding's shell broker.
            self.obj().activate();
            let paths: Vec<std::path::PathBuf> = files.iter().filter_map(gio::File::path).collect();
            if !paths.is_empty() {
                crate::converter::ui::shell::send_to_shell(
                    self.mode.get(),
                    crate::converter::ui::shell::ShellMsg::AddFiles(paths),
                );
            }
        }

        fn startup(&self) {
            self.parent_startup();
            crate::shared::app_base::strip_decoration_icon();

            let app = self.obj();

            install_quit_action(&*app);
            install_static_accels(&*app, &[("app.quit", &["<Control>q"])]);
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}
}

glib::wrapper! {
    /// `adw::Application` subclass shared by both converter binaries.
    ///
    /// Owns the `app.quit` action, default keyboard accelerators, and
    /// handles GTK `open` events by dispatching dropped files into the
    /// active [`Window`](crate::converter::ui::window::Window).
    ///
    /// Construct with [`Application::new_with_mode`].
    pub struct Application(ObjectSubclass<imp::Application>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Application {
    /// Construct an [`Application`] bound to an explicit
    /// [`ConverterMode`] (stored per instance — no process global) and
    /// configure the GTK `application-id` to the audio or video variant
    /// accordingly.
    #[must_use]
    pub fn new_with_mode(mode: ConverterMode) -> Self {
        let app_id = if mode.is_audio() {
            APP_ID_AUDIO
        } else {
            APP_ID_VIDEO
        };

        let app: Self = glib::Object::builder()
            .property("application-id", app_id)
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .build();
        app.imp().mode.set(mode);
        app
    }

    /// The [`ConverterMode`] this application instance runs.
    #[must_use]
    pub fn mode(&self) -> ConverterMode {
        self.imp().mode.get()
    }
}
