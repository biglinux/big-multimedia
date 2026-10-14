//! `BigAudioPlayerApplication`: AdwApplication entry point for single-instance
//! activation, file-open, app-level actions (quit/about), accel registration.

use adw::prelude::*;
use adw::subclass::prelude::*;
use big_app_kit::desktop::install_quit_action;
use gtk::{gio, glib};

use crate::audio::ui::audio_shell::AudioShellInput;
use crate::config;
use crate::settings::keys;
use big_player_ui::playback_bar::AudioPlaybackBarOutput;

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

/// Playback controls offered on the dock icon's menu: each `.desktop` action
/// runs `big-audio-player --<option>`, which hands the matching application
/// action to the running player and exits. `(option, action, help)`.
const DOCK_CONTROLS: &[(&str, &str, &str)] = &[
    (
        "play-pause",
        "play-pause",
        "Play or pause the running player",
    ),
    (
        "next",
        "next-track",
        "Skip to the next track in the running player",
    ),
    (
        "previous",
        "prev-track",
        "Go back to the previous track in the running player",
    ),
];

/// The shell message behind one dock control action.
fn dock_control_message(action: &str) -> Option<AudioShellInput> {
    let command = match action {
        "play-pause" => AudioPlaybackBarOutput::TogglePlay,
        "next-track" => AudioPlaybackBarOutput::Next,
        "prev-track" => AudioPlaybackBarOutput::Previous,
        _ => return None,
    };
    Some(AudioShellInput::Playback(command))
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

        /// `--play-pause`, `--next` and `--previous` from the dock menu: with
        /// a player already open, hand it the action over D-Bus and exit
        /// without a window of our own; otherwise start the player as usual.
        fn handle_local_options(
            &self,
            options: &glib::VariantDict,
        ) -> std::ops::ControlFlow<glib::ExitCode> {
            let Some(action) = DOCK_CONTROLS
                .iter()
                .find(|(option, _, _)| options.contains(option))
                .map(|(_, action, _)| *action)
            else {
                return self.parent_handle_local_options(options);
            };
            let app = self.obj();
            if app.register(gio::Cancellable::NONE).is_ok() && app.is_remote() {
                app.activate_action(action, None);
                // The call is queued on the bus: send it before exiting.
                if let Some(connection) = app.dbus_connection() {
                    let _ = connection.flush_sync(gio::Cancellable::NONE);
                }
                return std::ops::ControlFlow::Break(glib::ExitCode::SUCCESS);
            }
            std::ops::ControlFlow::Continue(())
        }

        fn startup(&self) {
            self.parent_startup();
            big_gtk_kit::install();
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
            for (_, action, _) in DOCK_CONTROLS {
                let control = gio::SimpleAction::new(action, None);
                control.connect_activate(|action, _| {
                    if let Some(message) = dock_control_message(&action.name()) {
                        crate::audio::ui::audio_shell::send_to_shell(message);
                    }
                });
                app.add_action(&control);
            }
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
        let app: Self = glib::Object::builder()
            .property("application-id", config::audio::APP_ID)
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .build();
        for (option, _, description) in DOCK_CONTROLS {
            app.add_main_option(
                option,
                glib::Char::from(0),
                glib::OptionFlags::NONE,
                glib::OptionArg::None,
                description,
                None,
            );
        }
        app
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
    fn every_dock_menu_action_runs_a_known_option() {
        let desktop =
            include_str!("../../resources/applications/br.com.biglinux.AudioPlayer.desktop");
        let execs: Vec<&str> = desktop
            .lines()
            .filter_map(|line| line.strip_prefix("Exec=big-audio-player --"))
            .collect();
        assert_eq!(execs.len(), DOCK_CONTROLS.len());
        for option in execs {
            let (_, action, _) = DOCK_CONTROLS
                .iter()
                .find(|(known, _, _)| *known == option)
                .unwrap_or_else(|| panic!("--{option} is not an option of the player"));
            assert!(
                dock_control_message(action).is_some(),
                "{action} does nothing"
            );
        }
    }

    #[test]
    fn open_paths_message_ignores_empty_inputs() {
        assert!(open_paths_message(Vec::new()).is_none());
    }

    #[test]
    fn open_paths_keeps_local_order() {
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
    fn open_files_skips_empty_and_remote() {
        assert!(open_files_message(&[]).is_none());
        let remote_file = gio::File::for_uri("https://example.invalid/audio.ogg");
        assert!(open_files_message(&[remote_file]).is_none());
    }

    #[test]
    #[cfg(not(miri))]
    fn open_files_keeps_local_order() {
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
    fn about_dialog_names_app_and_credits() {
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
    fn app_accels_skip_single_keys() {
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
