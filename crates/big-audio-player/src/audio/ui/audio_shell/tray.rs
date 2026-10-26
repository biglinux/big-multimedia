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

/// The tray icon, when the setting is on; `None` also without a session bus.
pub(super) fn build_tray(sender: &ComponentSender<AudioShell>, enabled: bool) -> Option<Tray> {
    if !enabled {
        return None;
    }
    let input = sender.input_sender().clone();
    let command: TrayCallback = std::rc::Rc::new(move |command| {
        let _ = input.send(AudioShellInput::Tray(command));
    });
    Tray::new(tray_spec(), command)
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
        assert!(svg.is_file(), "{} is missing", svg.display());
        let recipe = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../big-audio-player/PKGBUILD");
        let recipe = std::fs::read_to_string(&recipe).unwrap();
        assert!(
            recipe.contains(&format!("symbolic/apps/{icon}.svg")),
            "the package does not install {icon}"
        );
    }
}
