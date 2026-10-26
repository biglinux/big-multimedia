//! Video player `adw::Application` subclass: single-instance activation,
//! file-open (delivered to the Relm4 `VideoShell` via its thread-local
//! broker), startup (decoration-icon strip, color-scheme, actions, accels).
//! `--new-window` env opt-in flips `NON_UNIQUE` for multi-instance debugging.

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
        const NAME: &'static str = "BigMediaPlayerApplication";
        type Type = super::Application;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for Application {}

    impl ApplicationImpl for Application {
        // No `activate` override: the default emits the `activate` signal that
        // relm4's `RelmApp::run` connects to (it creates the VideoShell window
        // at startup and shows it on activate). Overriding without chaining
        // would suppress that signal and the window would never appear.

        fn open(&self, files: &[gio::File], _hint: &str) {
            // Show the relm4-managed window, then deliver opened files through
            // the shell's thread-local broker. `activate` is synchronous, so the
            // broker is set (in VideoShell::init) before the send.
            // A stream has no local path: keep its URL.
            let paths: Vec<String> = files
                .iter()
                .map(|f| {
                    f.path()
                        .map_or_else(|| f.uri().to_string(), |p| p.to_string_lossy().into_owned())
                })
                .collect();
            if !paths.is_empty() {
                crate::video::ui::video_shell::mark_opening_files();
            }
            self.obj().activate();
            crate::open_paths(paths);
        }

        fn startup(&self) {
            self.parent_startup();
            big_gtk_kit::install();
            crate::shared::app_base::strip_decoration_icon();

            let settings = crate::settings::Settings::for_video();
            crate::shared::app_base::apply_startup_color_scheme(&settings, keys::UI_COLOR_SOURCE);

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
                            app_name: config::video::APP_NAME,
                            app_icon: config::video::ICON_NAME,
                            comments: "",
                        },
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
            app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
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
        let mut flags = gio::ApplicationFlags::HANDLES_OPEN;
        if std::env::args().any(|a| a == "--new-window") {
            flags |= gio::ApplicationFlags::NON_UNIQUE;
        }
        glib::Object::builder()
            .property("application-id", config::video::APP_ID)
            .property("flags", flags)
            .build()
    }

    pub fn run(&self) -> glib::ExitCode {
        ApplicationExtManual::run(self)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn about_icon_is_the_icon_the_package_installs() {
        let icon = crate::config::video::ICON_NAME;
        let desktop =
            include_str!("../../resources/applications/br.com.biglinux.VideoPlayer.desktop");
        assert!(desktop.lines().any(|line| line == format!("Icon={icon}")));
        let svg = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("resources/icons/hicolor/scalable/apps/{icon}.svg"));
        assert!(svg.is_file(), "{} is missing", svg.display());
    }
}
