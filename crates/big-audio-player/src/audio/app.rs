//! `BigAudioPlayerApplication`: AdwApplication entry point — single-instance
//! activation, file-open, app-level actions (quit/about), accel registration.

use adw::prelude::*;
use adw::subclass::prelude::*;
use big_app_kit::desktop::install_quit_action;
use gtk::{gio, glib};

use crate::config;
use crate::settings::keys;

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
            let paths: Vec<String> = files
                .iter()
                .filter_map(|f| f.path().map(|p| p.to_string_lossy().into_owned()))
                .collect();
            if !paths.is_empty() {
                crate::audio::ui::audio_shell::send_to_shell(
                    crate::audio::ui::audio_shell::AudioShellInput::OpenPaths(paths),
                );
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
            Self::setup_accels(&app);
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}

    impl Application {
        fn setup_actions(app: &super::Application) {
            let about = gio::ActionEntry::builder("about")
                .activate(|app: &super::Application, _, _| {
                    let win = app.active_window();
                    let dialog = crate::shared::app_base::build_about_dialog(
                        &crate::shared::app_base::AboutDialogConfig {
                            app_name: config::audio::APP_NAME,
                            app_icon: config::audio::APP_ID,
                            comments: "Lyrics: LRCLIB · lyrics.ovh\n\
                                       Metadata: MusicBrainz\n\
                                       Scrobbling: Last.fm · ListenBrainz\n\
                                       Radio: Radio Browser\n\
                                       Art: Cover Art Archive",
                        },
                    );
                    dialog.add_credit_section(
                        Some("Data Sources"),
                        &[
                            "LRCLIB https://lrclib.net",
                            "lyrics.ovh https://lyrics.ovh",
                            "MusicBrainz https://musicbrainz.org",
                            "Cover Art Archive https://coverartarchive.org",
                            "Radio Browser https://www.radio-browser.info",
                            "ListenBrainz https://listenbrainz.org",
                        ],
                    );
                    if let Some(w) = win.as_ref() {
                        dialog.present(Some(w));
                    }
                })
                .build();
            install_quit_action(app);
            app.add_action_entries([about]);
        }

        fn setup_accels(app: &super::Application) {
            use crate::audio::keybindings::{resolve_accels, SHORTCUTS};
            use crate::settings::{keys, Settings};

            let custom_str = Settings::for_audio().get_string(keys::CUSTOM_KEYBINDINGS);

            // Multi-key defs go through GTK accels; single-key defs are
            // dispatched by `EventControllerKey` in the window — registering
            // bare letters as accels would fire them while typing in entries.
            for (def, accel) in resolve_accels(SHORTCUTS, &custom_str) {
                if def.single_key {
                    continue;
                }
                let full_action = if def.action.contains('.') {
                    def.action.to_string()
                } else {
                    format!("win.{}", def.action)
                };
                app.set_accels_for_action(&full_action, &[&accel]);
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
        glib::Object::builder()
            .property("application-id", config::audio::APP_ID)
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .build()
    }

    pub fn run(&self) -> glib::ExitCode {
        ApplicationExtManual::run(self)
    }
}
