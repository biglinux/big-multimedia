//! Window-level keyboard shortcuts for the video player.
//!
//! [`crate::video::keybindings::SHORTCUTS`] is the display/rebind catalog; this
//! module is what makes the bare-key bindings actually *do* something. An
//! `EventControllerKey` on the content root maps each pressed accelerator to its
//! action id and dispatches it: menu/toggle actions go through
//! [`VideoShellInput`], playback primitives (seek / volume / speed / frame-step)
//! drive the live mpv backend directly. Mirrors the audio player's controller
//! (`big_audio_player::audio::ui::audio_shell` key handler) — the video port had
//! lost this dispatch entirely (the table was display-only).

use super::*;

use crate::video::keybindings::{build_single_key_map, key_to_accel_string};

const SEEK_SHORT_SECS: f64 = 5.0;
const SEEK_LONG_SECS: f64 = 60.0;
const VOLUME_STEP: f64 = 5.0;
const SPEED_STEP: f64 = 0.1;
const SPEED_MIN: f64 = 0.25;
const SPEED_MAX: f64 = 4.0;

/// Install the window-level keyboard-shortcut controller on the player content
/// root. Bare single-key bindings only, skipped while a text widget has focus.
pub(super) fn wire_keyboard_shortcuts(
    root: &adw::ToastOverlay,
    mpv: &Rc<RefCell<Option<Arc<MpvBackend>>>>,
    settings: &Settings,
    sender: &ComponentSender<VideoShell>,
) {
    let key_controller = gtk::EventControllerKey::new();
    let single_key_actions = build_single_key_map(&settings.get_string(keys::CUSTOM_KEYBINDINGS));
    let mpv = mpv.clone();
    let input = sender.input_sender().clone();
    let settings = settings.clone();
    key_controller.connect_key_pressed(move |controller, keyval, _code, state| {
        // Read the same volume ceiling the slider uses, so the keyboard step
        // reaches the >100% boost region instead of a hardcoded 100 cap.
        let volume_max = f64::from(settings.get_int(keys::VOLUME_MAX)).max(100.0);
        // Hardware media keys fire regardless of focus.
        if let Some(action) = media_key_action(keyval)
            && dispatch_video_shortcut(action, &mpv, &input, volume_max)
        {
            return glib::Propagation::Stop;
        }
        if focus_accepts_text(controller) {
            return glib::Propagation::Proceed;
        }
        let accel = key_to_accel_string(keyval, state);
        let Some(action) = single_key_actions.get(accel.as_str()) else {
            return glib::Propagation::Proceed;
        };
        if dispatch_video_shortcut(action, &mpv, &input, volume_max) {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    root.add_controller(key_controller);
}

/// Install the capture-phase trim-mode key controller (M = mark, Esc = exit). It
/// runs before the normal bubble-phase shortcut controller and acts only while
/// trim mode is active, so outside trim mode those keys keep their usual meaning.
/// A handled key is consumed (`Stop`) so it never reaches the normal map.
pub(super) fn wire_trim_keys(
    root: &adw::ToastOverlay,
    trim_active: Rc<Cell<bool>>,
    sender: &ComponentSender<VideoShell>,
) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let input = sender.input_sender().clone();
    controller.connect_key_pressed(move |controller, keyval, _code, _state| {
        use VideoShellInput as In;
        use gtk::gdk::Key;
        if !trim_active.get() || focus_accepts_text(controller) {
            return glib::Propagation::Proceed;
        }
        let message = match keyval {
            Key::m | Key::M => In::TrimMark,
            Key::Escape => In::ExitTrimMode,
            _ => return glib::Propagation::Proceed,
        };
        let _ = input.send(message);
        glib::Propagation::Stop
    });
    root.add_controller(controller);
}

/// Map a shortcut action id to its effect. Menu/toggle actions are sent as shell
/// messages; playback primitives drive the live mpv backend directly. Returns
/// `false` for actions with no live target (e.g. mpv not up) so the key event
/// keeps propagating.
fn dispatch_video_shortcut(
    action: &str,
    mpv: &Rc<RefCell<Option<Arc<MpvBackend>>>>,
    input: &relm4::Sender<VideoShellInput>,
    volume_max: f64,
) -> bool {
    use VideoShellInput as In;
    let message = match action {
        "play-pause" => Some(In::TogglePlayback),
        "fullscreen" | "fullscreen-f11" | "exit-fullscreen" => Some(In::ToggleFullscreen),
        "screenshot" => Some(In::Screenshot),
        "goto-time" => Some(In::ShowGotoTime),
        "media-info" => Some(In::ShowMediaInfo),
        "open-file" => Some(In::ShowOpenFile),
        "open-url" => Some(In::ShowOpenUrl),
        "toggle-files-sidebar" => Some(In::ToggleFilesSidebar),
        "toggle-playlist-sidebar" => Some(In::TogglePlaylistSidebar),
        "playlist-next" => Some(In::Controls(VideoControlsBarOutput::Next)),
        "playlist-prev" => Some(In::Controls(VideoControlsBarOutput::Previous)),
        _ => None,
    };
    if let Some(message) = message {
        let _ = input.send(message);
        return true;
    }

    // Playback primitives: drive the backend directly (no shell round-trip).
    let Some(mpv) = mpv.borrow().as_ref().cloned() else {
        return false;
    };
    match action {
        "seek-forward" => mpv.seek(SEEK_SHORT_SECS),
        "seek-backward" => mpv.seek(-SEEK_SHORT_SECS),
        "seek-forward-long" => mpv.seek(SEEK_LONG_SECS),
        "seek-backward-long" => mpv.seek(-SEEK_LONG_SECS),
        "volume-up" => mpv.set_volume((mpv.get_volume() + VOLUME_STEP).min(volume_max)),
        "volume-down" => mpv.set_volume((mpv.get_volume() - VOLUME_STEP).max(0.0)),
        "mute" => mpv.toggle_mute(),
        "speed-up" => mpv.set_speed((mpv.get_speed() + SPEED_STEP).min(SPEED_MAX)),
        "speed-down" => mpv.set_speed((mpv.get_speed() - SPEED_STEP).max(SPEED_MIN)),
        "speed-reset" => mpv.set_speed(1.0),
        "frame-step" => mpv.frame_step(),
        "frame-back-step" => mpv.frame_back_step(),
        _ => return false,
    }
    true
}

/// Map hardware media keys to shortcut action ids.
fn media_key_action(keyval: gtk::gdk::Key) -> Option<&'static str> {
    use gtk::gdk::Key;
    match keyval {
        Key::AudioPlay | Key::AudioPause | Key::AudioMedia => Some("play-pause"),
        Key::AudioNext => Some("playlist-next"),
        Key::AudioPrev => Some("playlist-prev"),
        _ => None,
    }
}

/// True when the focused widget is a text editor — bare-key shortcuts must yield
/// so typing works (mirrors the audio player's `focus_accepts_text`).
fn focus_accepts_text(controller: &gtk::EventControllerKey) -> bool {
    let Some(widget) = controller.widget() else {
        return false;
    };
    let Some(window) = widget.root().and_downcast::<gtk::Window>() else {
        return false;
    };
    let Some(focus) = gtk::prelude::GtkWindowExt::focus(&window) else {
        return false;
    };
    focus.dynamic_cast_ref::<gtk::Editable>().is_some()
        || focus.is::<gtk::TextView>()
        || focus.ancestor(gtk::TextView::static_type()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_mpv() -> Rc<RefCell<Option<Arc<MpvBackend>>>> {
        Rc::new(RefCell::new(None))
    }

    #[test]
    fn menu_and_toggle_actions_dispatch_without_mpv() {
        let (tx, _rx) = relm4::channel::<VideoShellInput>();
        let mpv = no_mpv();
        for action in [
            "play-pause",
            "fullscreen",
            "exit-fullscreen",
            "screenshot",
            "goto-time",
            "media-info",
            "open-file",
            "open-url",
            "toggle-files-sidebar",
            "toggle-playlist-sidebar",
            "playlist-next",
            "playlist-prev",
        ] {
            assert!(
                dispatch_video_shortcut(action, &mpv, &tx, 150.0),
                "{action} should dispatch a shell message"
            );
        }
    }

    #[test]
    fn playback_primitives_report_unhandled_without_mpv() {
        // No backend → the primitive can't act, so the key event must keep
        // propagating (return false), never silently swallowed.
        let (tx, _rx) = relm4::channel::<VideoShellInput>();
        let mpv = no_mpv();
        for action in [
            "seek-forward",
            "seek-backward",
            "volume-up",
            "volume-down",
            "mute",
            "speed-up",
            "speed-reset",
            "frame-step",
        ] {
            assert!(
                !dispatch_video_shortcut(action, &mpv, &tx, 150.0),
                "{action} needs a live backend"
            );
        }
    }

    #[test]
    fn unknown_action_is_unhandled() {
        let (tx, _rx) = relm4::channel::<VideoShellInput>();
        assert!(!dispatch_video_shortcut(
            "does-not-exist",
            &no_mpv(),
            &tx,
            150.0
        ));
    }

    #[test]
    fn single_key_map_binds_core_player_keys() {
        let map = build_single_key_map("");
        assert_eq!(map.get("space").map(String::as_str), Some("play-pause"));
        assert_eq!(map.get("f").map(String::as_str), Some("fullscreen"));
        assert_eq!(
            map.get("F9").map(String::as_str),
            Some("toggle-files-sidebar")
        );
        assert_eq!(
            map.get("<Shift>F9").map(String::as_str),
            Some("toggle-playlist-sidebar")
        );
        assert_eq!(map.get("s").map(String::as_str), Some("screenshot"));
    }
}
