//! `BigAudioPlayerApplication`: AdwApplication entry point — single-instance
//! activation, file-open, app-level actions (quit/about), accel registration.

use adw::prelude::*;
use adw::subclass::prelude::*;
use big_app_kit::desktop::install_quit_action;
use gtk::{gio, glib};

use crate::audio::ui::audio_shell::AudioShellInput;
use crate::config;
use crate::settings::keys;

const AUDIO_ABOUT_COMMENTS: &str = "Lyrics: LRCLIB · lyrics.ovh\n\
                                   Metadata: MusicBrainz\n\
                                   Radio: Radio Browser\n\
                                   Art: Cover Art Archive";
const AUDIO_ABOUT_CREDIT_SECTION_HEADING: &str = "Data Sources";
const AUDIO_ABOUT_CREDIT_LINES: &[&str] = &[
    "LRCLIB https://lrclib.net",
    "lyrics.ovh https://lyrics.ovh",
    "MusicBrainz https://musicbrainz.org",
    "Cover Art Archive https://coverartarchive.org",
    "Radio Browser https://www.radio-browser.info",
    "ListenBrainz https://listenbrainz.org",
];

pub(crate) fn open_paths_message(paths: Vec<String>) -> Option<AudioShellInput> {
    if paths.is_empty() {
        None
    } else {
        Some(AudioShellInput::OpenPaths(paths))
    }
}

fn open_files_message(files: &[gio::File]) -> Option<AudioShellInput> {
    let paths: Vec<String> = files
        .iter()
        .filter_map(|file| file.path().map(|path| path.to_string_lossy().into_owned()))
        .collect();
    open_paths_message(paths)
}

fn audio_about_dialog_config() -> crate::shared::app_base::AboutDialogConfig {
    crate::shared::app_base::AboutDialogConfig {
        app_name: config::audio::APP_NAME,
        app_icon: config::audio::APP_ID,
        comments: AUDIO_ABOUT_COMMENTS,
    }
}

fn audio_accel_action_name(action: &str) -> String {
    if action.contains('.') {
        action.to_string()
    } else {
        format!("win.{action}")
    }
}

fn resolved_audio_application_accels(custom_shortcuts: &str) -> Vec<(String, String)> {
    use crate::audio::keybindings::{SHORTCUTS, resolve_accels};

    resolve_accels(SHORTCUTS, custom_shortcuts)
        .into_iter()
        .filter(|(definition, _)| !definition.single_key)
        .map(|(definition, accel)| (audio_accel_action_name(definition.action), accel))
        .collect()
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Application;

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "BigAudioPlayerApplication";
        type Type = super::Application;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for Application {}

    impl ApplicationImpl for Application {
        // No `activate` override: the default emits the `activate` signal that
        // relm4's `RelmApp::run` connects to (it creates the AudioShell window
        // at startup and shows it on activate). Overriding without chaining
        // would suppress that signal and the window would never appear.

        fn open(&self, files: &[gio::File], _hint: &str) {
            // Show the relm4-managed window, then deliver opened files through
            // the shell's thread-local broker.
            self.obj().activate();
            if let Some(message) = open_files_message(files) {
                crate::audio::ui::audio_shell::send_to_shell(message);
            }
        }

        fn startup(&self) {
            self.parent_startup();
            crate::shared::app_base::strip_decoration_icon();
            let settings = crate::settings::Settings::for_audio();
            crate::shared::app_base::apply_startup_color_scheme(
                &settings,
                keys::AUDIO_UI_COLOR_SOURCE,
            );

            let app = self.obj();
            Self::setup_actions(&app);
            super::install_audio_accels(app.upcast_ref());
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}

    impl Application {
        fn setup_actions(app: &super::Application) {
            let about = gio::ActionEntry::builder("about")
                .activate(|app: &super::Application, _, _| {
                    let win = app.active_window();
                    let dialog =
                        crate::shared::app_base::build_about_dialog(&audio_about_dialog_config());
                    dialog.add_credit_section(
                        Some(AUDIO_ABOUT_CREDIT_SECTION_HEADING),
                        AUDIO_ABOUT_CREDIT_LINES,
                    );
                    if let Some(w) = win.as_ref() {
                        dialog.present(Some(w));
                    }
                })
                .build();
            install_quit_action(app);
            app.add_action_entries([about]);
        }
    }
}

glib::wrapper! {
    pub struct Application(ObjectSubclass<imp::Application>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Application {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", config::audio::APP_ID)
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .build()
    }

    pub fn run(&self) -> glib::ExitCode {
        ApplicationExtManual::run(self)
    }
}

/// Install the audio player's configured GTK accelerators on the supplied app.
///
/// Standalone startup calls this through [`Application`]; `big-host` modules
/// call it during embed boot because they reuse the host `gtk::Application`.
pub(crate) fn install_audio_accels(app: &gtk::Application) {
    use crate::settings::{Settings, keys};

    let custom_str = Settings::for_audio().get_string(keys::CUSTOM_KEYBINDINGS);

    // Multi-key defs go through GTK accels; single-key defs are dispatched by
    // `EventControllerKey` in the window so text widgets keep normal typing.
    for (action, accel) in resolved_audio_application_accels(&custom_str) {
        app.set_accels_for_action(&action, &[&accel]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_paths_message_ignores_empty_inputs() {
        assert!(open_paths_message(Vec::new()).is_none());
    }

    #[test]
    fn open_paths_message_preserves_local_paths_in_order() {
        match open_paths_message(vec![
            "/tmp/first.ogg".to_string(),
            "/tmp/second.flac".to_string(),
        ]) {
            Some(AudioShellInput::OpenPaths(paths)) => {
                assert_eq!(paths, ["/tmp/first.ogg", "/tmp/second.flac"]);
            }
            other => panic!("expected OpenPaths shell message, got {other:?}"),
        }
    }

    #[test]
    #[cfg(not(miri))]
    fn open_files_message_ignores_empty_and_non_local_inputs() {
        assert!(open_files_message(&[]).is_none());
        let remote_file = gio::File::for_uri("https://example.invalid/audio.ogg");
        assert!(open_files_message(&[remote_file]).is_none());
    }

    #[test]
    #[cfg(not(miri))]
    fn open_files_message_preserves_local_paths_in_order() {
        let first_file = gio::File::for_path("/tmp/first.ogg");
        let second_file = gio::File::for_path("/tmp/second.flac");

        match open_files_message(&[first_file, second_file]) {
            Some(AudioShellInput::OpenPaths(paths)) => {
                assert_eq!(paths, ["/tmp/first.ogg", "/tmp/second.flac"]);
            }
            other => panic!("expected OpenPaths shell message, got {other:?}"),
        }
    }

    #[test]
    fn about_dialog_contract_names_app_and_credit_sources() {
        let config = audio_about_dialog_config();

        assert_eq!(config.app_name, crate::config::audio::APP_NAME);
        assert_eq!(config.app_icon, crate::config::audio::APP_ID);
        assert!(config.comments.contains("MusicBrainz"));
        assert_eq!(AUDIO_ABOUT_CREDIT_SECTION_HEADING, "Data Sources");
        assert!(
            AUDIO_ABOUT_CREDIT_LINES
                .iter()
                .any(|source| source.contains("Radio Browser"))
        );
    }

    #[test]
    fn audio_application_accels_skip_single_key_and_prefix_window_actions() {
        let actions: std::collections::HashSet<String> = resolved_audio_application_accels("")
            .into_iter()
            .map(|(action, _)| action)
            .collect();

        assert!(actions.contains("app.quit"));
        assert!(actions.contains("win.next-track"));
        assert!(actions.contains("win.prev-track"));
        assert!(!actions.contains("win.play-pause"));
    }

    #[test]
    fn audio_accel_action_name_preserves_app_scope() {
        assert_eq!(audio_accel_action_name("app.quit"), "app.quit");
        assert_eq!(
            audio_accel_action_name("toggle-repeat"),
            "win.toggle-repeat"
        );
    }
}
