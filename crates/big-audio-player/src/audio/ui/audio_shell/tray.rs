//! System tray icon, shown only when Preferences asks for it.

use super::*;
use big_app_kit::tray::{Tray, TrayCallback, TrayCommand, TrayMenuItem, TraySpec};

const SHOW: &str = "show";
const PLAY_PAUSE: &str = "play-pause";
const NEXT: &str = "next";
const PREVIOUS: &str = "previous";
const QUIT: &str = "quit";

fn tray_spec() -> TraySpec {
    TraySpec {
        id: crate::config::audio::CONFIG_DIR.to_owned(),
        title: crate::config::audio::APP_NAME.to_owned(),
        icon_name: crate::config::audio::TRAY_ICON_NAME.to_owned(),
        items: vec![
            TrayMenuItem::action(SHOW, i18n("Show Big Audio Player")),
            TrayMenuItem::Separator,
            TrayMenuItem::action(PLAY_PAUSE, i18n("Play / Pause"))
                .with_icon("media-playback-start-symbolic"),
            TrayMenuItem::action(NEXT, i18n("Next Track")).with_icon("media-skip-forward-symbolic"),
            TrayMenuItem::action(PREVIOUS, i18n("Previous Track"))
                .with_icon("media-skip-backward-symbolic"),
            TrayMenuItem::Separator,
            TrayMenuItem::action(QUIT, i18n("Quit")).with_icon("application-exit-symbolic"),
        ],
    }
}

thread_local! {
    /// While the icon is on: whether a tray host shows it.
    static TRAY_SHOWN: std::cell::RefCell<Option<std::rc::Rc<std::cell::Cell<bool>>>> =
        const { std::cell::RefCell::new(None) };
    /// The person asked to quit: the close that follows ends the player.
    static QUITTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether closing the window should hide it to the tray instead: the icon
/// is on, a tray host shows it (without one the window could not come back),
/// and no quit was asked for.
pub(crate) fn close_hides_window() -> bool {
    !QUITTING.get()
        && TRAY_SHOWN.with_borrow(|shown| shown.as_ref().is_some_and(|shown| shown.get()))
}

/// The next close ends the player even with the tray icon on.
pub(super) fn mark_quitting() {
    QUITTING.set(true);
}

/// The tray icon, when the setting is on; `None` also without a session bus.
pub(super) fn build_tray(sender: &ComponentSender<AudioShell>, enabled: bool) -> Option<Tray> {
    let tray = enabled.then(|| {
        let input = sender.input_sender().clone();
        let command: TrayCallback = std::rc::Rc::new(move |command| {
            let _ = input.send(AudioShellInput::Tray(command));
        });
        Tray::new(tray_spec(), command)
    });
    let tray = tray.flatten();
    TRAY_SHOWN.set(tray.as_ref().map(Tray::shown_flag));
    tray
}

/// The transport control behind a menu entry, the same the dock menu sends.
fn playback_command(action: &str) -> Option<AudioPlaybackBarOutput> {
    Some(match action {
        PLAY_PAUSE => AudioPlaybackBarOutput::TogglePlay,
        NEXT => AudioPlaybackBarOutput::Next,
        PREVIOUS => AudioPlaybackBarOutput::Previous,
        _ => return None,
    })
}

impl AudioShell {
    pub(super) fn handle_tray_command(
        &mut self,
        command: TrayCommand,
        sender: &ComponentSender<Self>,
    ) {
        match command {
            // A click on the icon hides the window in front, shows it otherwise.
            TrayCommand::Activate => {
                if let Some(window) = self.window() {
                    if window.is_visible() && window.is_active() {
                        window.set_visible(false);
                    } else {
                        window.present();
                    }
                }
            }
            TrayCommand::SecondaryActivate => {
                self.handle_playback(AudioPlaybackBarOutput::TogglePlay, sender);
            }
            TrayCommand::Menu(action) => match action.as_str() {
                SHOW => {
                    if let Some(window) = self.window() {
                        window.present();
                    }
                }
                QUIT => self.handle_mpris_command(MprisCommand::Quit, sender),
                _ => {
                    if let Some(out) = playback_command(&action) {
                        self.handle_playback(out, sender);
                    }
                }
            },
        }
    }

    /// Apply the tray switch of Preferences without a restart.
    pub(super) fn set_tray_icon(&mut self, enabled: bool, sender: &ComponentSender<Self>) {
        if enabled == self.tray.is_some() {
            return;
        }
        self.tray = build_tray(sender, enabled);
        // Without the icon, a window hidden from it could not come back.
        if !enabled
            && let Some(window) = self.window()
            && !window.is_visible()
        {
            window.present();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_hides_only_with_a_shown_icon_and_no_quit() {
        assert!(!close_hides_window(), "no icon: close ends the player");
        let shown = std::rc::Rc::new(std::cell::Cell::new(false));
        TRAY_SHOWN.set(Some(shown.clone()));
        assert!(
            !close_hides_window(),
            "no tray host: the window could not come back"
        );
        shown.set(true);
        assert!(close_hides_window());
        mark_quitting();
        assert!(!close_hides_window(), "Quit ends the player");
    }

    #[test]
    fn every_menu_entry_does_something() {
        for item in tray_spec().items {
            if let TrayMenuItem::Action { action, .. } = item {
                assert!(
                    [SHOW, QUIT].contains(&action.as_str()) || playback_command(&action).is_some(),
                    "{action} does nothing"
                );
            }
        }
    }

    #[test]
    fn tray_icon_is_installed_as_a_symbolic_icon() {
        let icon = crate::config::audio::TRAY_ICON_NAME;
        assert!(
            icon.ends_with("-symbolic"),
            "the panel recolors only symbolic icons"
        );
        let svg = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("resources/icons/hicolor/symbolic/apps/{icon}.svg"));
        let head = std::fs::read(&svg).unwrap();
        // gdk-pixbuf tells an SVG by `<svg` near the top: anything long
        // before it (a comment) makes the panel fail to load the icon.
        assert!(
            head.windows(4).take(256).any(|w| w == b"<svg"),
            "{} must open its <svg> element within the first 256 bytes",
            svg.display()
        );
        let recipe = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../big-audio-player/PKGBUILD");
        let recipe = std::fs::read_to_string(&recipe).unwrap();
        assert!(
            recipe.contains(&format!("symbolic/apps/{icon}.svg")),
            "the package does not install {icon}"
        );
    }
}
