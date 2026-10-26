//! Window-level keyboard shortcuts for the video player.
//!
//! [`crate::video::keybindings::SHORTCUTS`] is the display/rebind catalog; this
//! module is what makes the bare-key bindings actually *do* something. An
//! `EventControllerKey` on the content root maps each pressed accelerator to its
//! action id and dispatches it: menu/toggle actions go through
//! [`VideoShellInput`], playback primitives (seek / volume / speed / frame-step)
//! drive the live mpv backend directly. Mirrors the audio player's controller
//! (`big_audio_player::audio::ui::audio_shell` key handler).

use super::*;

use crate::video::keybindings::build_single_key_map;
use big_app_kit::keybindings::focus_accepts_text;

const SEEK_SHORT_SECS: f64 = 5.0;
/// Shift+arrow seek; the shortcuts dialog calls it 30 s.
const SEEK_LONG_SECS: f64 = 30.0;
const VOLUME_STEP: f64 = 5.0;
const SPEED_STEP: f64 = 0.1;
const SPEED_MIN: f64 = 0.25;
const SPEED_MAX: f64 = 4.0;
const PANSCAN_STEP: f64 = 0.1;

/// Install the keyboard shortcuts and the trim-mode keys (M = mark, Esc =
/// exit).
///
/// Controllers on the content root see a key only when the focus is inside
/// it. Clicking the video focuses nothing, and the focus also goes away when
/// the auto-hidden OSD takes the focused button with it: the key then goes to
/// the window alone, and the window's own bindings eat Space and the arrows.
/// A capture-phase controller on the window handles exactly that case, the
/// window with no focus widget, so a dialog or a sidebar list holding the
/// focus keeps its keys. It also takes Space from a button of the OSD
/// (`osd_areas`), where it would press the button again instead of pausing;
/// Enter still presses it.
pub(super) fn wire_keyboard_shortcuts(
    root: &gtk::Box,
    mpv: &Rc<RefCell<Option<Arc<MpvBackend>>>>,
    tv: &Rc<crate::shared::tv_cast::TvLink>,
    settings: &Settings,
    sender: &ComponentSender<VideoShell>,
    trim_active: Rc<Cell<bool>>,
    popovers: &[gtk::Popover],
    osd_areas: &[gtk::Widget],
) {
    let input = sender.input_sender().clone();
    let shortcuts = Rc::new(Shortcuts {
        single_key_actions: RefCell::default(),
        mpv: mpv.clone(),
        tv: tv.clone(),
        input: input.clone(),
        settings: settings.clone(),
        popovers: popovers.iter().map(ObjectExt::downgrade).collect(),
    });

    // Trim keys run before the normal map, and only in trim mode, so outside
    // it M and Esc keep their usual meaning.
    let trim = Rc::new(move |keyval: gtk::gdk::Key| {
        use VideoShellInput as In;
        use gtk::gdk::Key;
        if !trim_active.get() {
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

    let trim_controller = gtk::EventControllerKey::new();
    trim_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    trim_controller.connect_key_pressed({
        let trim = trim.clone();
        move |controller, keyval, _code, _state| {
            if controller
                .widget()
                .is_some_and(|widget| focus_accepts_text(&widget))
            {
                return glib::Propagation::Proceed;
            }
            trim(keyval)
        }
    });
    root.add_controller(trim_controller);

    let key_controller = gtk::EventControllerKey::new();
    key_controller.connect_key_pressed({
        let shortcuts = shortcuts.clone();
        move |controller, keyval, _code, state| {
            let in_text = controller
                .widget()
                .is_some_and(|widget| focus_accepts_text(&widget));
            shortcuts.handle(keyval, state, in_text)
        }
    });
    root.add_controller(key_controller);

    let window_controller = gtk::EventControllerKey::new();
    window_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let osd_areas: Vec<glib::WeakRef<gtk::Widget>> =
        osd_areas.iter().map(ObjectExt::downgrade).collect();
    window_controller.connect_key_pressed(move |controller, keyval, _code, state| {
        let Some(window) = controller.widget().and_downcast::<gtk::Window>() else {
            return glib::Propagation::Proceed;
        };
        match gtk::prelude::GtkWindowExt::focus(&window) {
            None => {
                if trim(keyval) == glib::Propagation::Stop {
                    return glib::Propagation::Stop;
                }
                shortcuts.handle(keyval, state, false)
            }
            Some(focus)
                if is_osd_button(&focus, &osd_areas)
                    && shortcuts.action_for(keyval, state).as_deref() == Some("play-pause") =>
            {
                shortcuts.handle(keyval, state, false)
            }
            Some(_) => glib::Propagation::Proceed,
        }
    });
    big_app_kit::keybindings::attach_to_window(root, window_controller);
}

/// A button on the header or the control bar, outside any of their menus.
fn is_osd_button(focus: &gtk::Widget, osd_areas: &[glib::WeakRef<gtk::Widget>]) -> bool {
    focus.is::<gtk::Button>()
        && focus.ancestor(gtk::Popover::static_type()).is_none()
        && osd_areas
            .iter()
            .filter_map(glib::WeakRef::upgrade)
            .any(|area| focus.is_ancestor(&area))
}

/// What the shortcut keys act on.
struct Shortcuts {
    /// The saved custom bindings and the key map built from them, rebuilt
    /// when the shortcuts dialog saves a change.
    single_key_actions: RefCell<(Option<String>, std::collections::HashMap<String, String>)>,
    mpv: Rc<RefCell<Option<Arc<MpvBackend>>>>,
    tv: Rc<crate::shared::tv_cast::TvLink>,
    input: relm4::Sender<VideoShellInput>,
    settings: Settings,
    /// The control-bar and header menus: Escape closes them first.
    popovers: Vec<glib::WeakRef<gtk::Popover>>,
}

impl Shortcuts {
    /// The action bound to a key press under the current bindings.
    fn action_for(&self, keyval: gtk::gdk::Key, state: gtk::gdk::ModifierType) -> Option<String> {
        let custom = self.settings.get_string(keys::CUSTOM_KEYBINDINGS);
        let mut cache = self.single_key_actions.borrow_mut();
        if cache.0.as_deref() != Some(custom.as_str()) {
            *cache = (Some(custom.clone()), build_single_key_map(&custom));
        }
        lookup_action(&cache.1, keyval, state).map(str::to_string)
    }

    fn handle(
        &self,
        keyval: gtk::gdk::Key,
        state: gtk::gdk::ModifierType,
        in_text: bool,
    ) -> glib::Propagation {
        // Read the same volume ceiling the slider uses, so the keyboard step
        // reaches the >100% boost region instead of a hardcoded 100 cap.
        let volume_max = f64::from(self.settings.get_int(keys::VOLUME_MAX)).max(100.0);
        // Hardware media keys fire regardless of focus.
        if let Some(action) = media_key_action(keyval)
            && dispatch_video_shortcut(action, &self.mpv, &self.tv, &self.input, volume_max)
        {
            return glib::Propagation::Stop;
        }
        if in_text {
            return glib::Propagation::Proceed;
        }
        if keyval == gtk::gdk::Key::Escape && close_open_popovers(&self.popovers) {
            return glib::Propagation::Stop;
        }
        let Some(action) = self.action_for(keyval, state) else {
            return glib::Propagation::Proceed;
        };
        if dispatch_video_shortcut(&action, &self.mpv, &self.tv, &self.input, volume_max) {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    }
}

/// Pop down every open menu in `popovers`; `true` when one was open.
fn close_open_popovers(popovers: &[glib::WeakRef<gtk::Popover>]) -> bool {
    let mut closed = false;
    for popover in popovers.iter().filter_map(glib::WeakRef::upgrade) {
        if popover.is_visible() {
            popover.popdown();
            closed = true;
        }
    }
    closed
}

fn lookup_action(
    map: &std::collections::HashMap<String, String>,
    keyval: gtk::gdk::Key,
    state: gtk::gdk::ModifierType,
) -> Option<&str> {
    big_app_kit::keybindings::lookup_shortcut(map, keyval, state)
}

/// Play what the clipboard holds: a stream URL, a `file://` URI or a path to
/// a file that exists.
pub(super) fn paste_and_play(root: &gtk::Box, sender: &ComponentSender<VideoShell>) {
    let clipboard = root.clipboard();
    let sender = sender.clone();
    glib::spawn_future_local(async move {
        let Ok(Some(text)) = clipboard.read_text_future().await else {
            return;
        };
        let text = text.trim();
        let target = super::dialogs::accepted_open_url(text).or_else(|| {
            let path = gtk::gio::File::for_commandline_arg(text).path()?;
            path.is_file().then(|| path.to_string_lossy().into_owned())
        });
        if let Some(target) = target {
            sender.input(VideoShellInput::OpenFiles(vec![target]));
        }
    });
}

/// Map a shortcut action id to its effect. Menu/toggle actions are sent as shell
/// messages; playback primitives drive the live mpv backend directly. Returns
/// `false` for actions with no live target (e.g. mpv not up) so the key event
/// keeps propagating.
fn dispatch_video_shortcut(
    action: &str,
    mpv: &Rc<RefCell<Option<Arc<MpvBackend>>>>,
    tv: &Rc<crate::shared::tv_cast::TvLink>,
    input: &relm4::Sender<VideoShellInput>,
    volume_max: f64,
) -> bool {
    use VideoShellInput as In;
    let message = match action {
        "play-pause" => Some(In::TogglePlayback),
        "fullscreen" | "fullscreen-f11" => Some(In::ToggleFullscreen),
        "exit-fullscreen" => Some(In::Escape),
        "screenshot" => Some(In::Screenshot),
        "goto-time" => Some(In::ShowGotoTime),
        "media-info" => Some(In::ShowMediaInfo),
        "open-file" => Some(In::ShowOpenFile),
        "open-url" => Some(In::ShowOpenUrl),
        "toggle-files-sidebar" => Some(In::ToggleFilesSidebar),
        "toggle-playlist-sidebar" => Some(In::TogglePlaylistSidebar),
        "playlist-next" => Some(In::Controls(VideoControlsBarOutput::Next)),
        "playlist-prev" => Some(In::Controls(VideoControlsBarOutput::Previous)),
        "crop" => Some(In::ShowPanel(super::panel::PanelPage::Framing)),
        "music-shuffle" => Some(In::ToggleShuffle),
        "music-repeat" => Some(In::CycleRepeat),
        "paste-play" => Some(In::PastePlay),
        _ => None,
    };
    if let Some(message) = message {
        let _ = input.send(message);
        return true;
    }

    // While a TV has the controls, seeking and volume keys drive it.
    if let Some(state) = tv.attached() {
        let seek = |offset: f64| {
            if state.can_seek {
                let target = (state.position + offset).clamp(0.0, state.duration.max(0.0));
                tv.control("seek-to", target);
            }
        };
        let volume = state.volume.unwrap_or(1.0);
        match action {
            "seek-forward" => seek(SEEK_SHORT_SECS),
            "seek-backward" => seek(-SEEK_SHORT_SECS),
            "seek-forward-long" => seek(SEEK_LONG_SECS),
            "seek-backward-long" => seek(-SEEK_LONG_SECS),
            "volume-up" => tv.control("volume", (volume + VOLUME_STEP / 100.0).min(1.0)),
            "volume-down" => tv.control("volume", (volume - VOLUME_STEP / 100.0).max(0.0)),
            "mute" if state.muted == Some(true) => tv.control("unmute", 0.0),
            "mute" => tv.control("mute", 0.0),
            // Speed and frame steps have no meaning on a TV stream.
            _ => return false,
        }
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
        "revert-seek" => mpv.revert_seek(),
        "chapter-next" | "chapter-prev" | "chapter-next-alt" | "chapter-prev-alt" => {
            let count = mpv.get_chapter_count();
            if count <= 0 {
                return false;
            }
            let step = if action.starts_with("chapter-next") {
                1
            } else {
                -1
            };
            mpv.set_chapter((mpv.get_current_chapter() + step).clamp(0, count - 1));
        }
        "deinterlace" => mpv.set_deinterlace(!mpv.get_property_bool("deinterlace")),
        "hwdec" => mpv.set_hwdec(if mpv.get_hwdec() == "no" {
            "auto-safe"
        } else {
            "no"
        }),
        "panscan-up" => mpv.set_panscan((mpv.get_panscan() + PANSCAN_STEP).min(1.0)),
        "panscan-down" => mpv.set_panscan((mpv.get_panscan() - PANSCAN_STEP).max(0.0)),
        "audio-passthrough" => mpv.toggle_audio_passthrough(),
        "secondary-sub" if mpv.has_secondary_sub() => mpv.disable_secondary_sub(),
        "secondary-sub" => mpv.cycle("secondary-sid"),
        "sub-seek-forward" => mpv.sub_seek(1),
        "sub-seek-backward" => mpv.sub_seek(-1),
        "cycle-cc" => mpv.cycle("sub"),
        "cycle-edition" => mpv.cycle("edition"),
        _ => return false,
    }
    // Speed and zoom show as "away from normal" in the settings panel.
    if action.starts_with("speed-") || action.starts_with("panscan-") {
        let _ = input.send(VideoShellInput::RefreshAdjustments);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn no_mpv() -> Rc<RefCell<Option<Arc<MpvBackend>>>> {
        Rc::new(RefCell::new(None))
    }

    /// Never connected, so no TV has the keys.
    fn no_tv() -> Rc<crate::shared::tv_cast::TvLink> {
        crate::shared::tv_cast::TvLink::new()
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
                dispatch_video_shortcut(action, &mpv, &no_tv(), &tx, 150.0),
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
                !dispatch_video_shortcut(action, &mpv, &no_tv(), &tx, 150.0),
                "{action} needs a live backend"
            );
        }
    }

    #[test]
    fn key_lookup_survives_caps_lock_and_shifted_symbols() {
        use gtk::gdk::{Key, ModifierType};
        let map = build_single_key_map("");
        let lookup = |key, state| lookup_action(&map, key, state);
        assert_eq!(
            lookup(Key::space, ModifierType::empty()),
            Some("play-pause")
        );
        // Caps Lock turns `f` into `F`.
        assert_eq!(lookup(Key::F, ModifierType::LOCK_MASK), Some("fullscreen"));
        // `>` and `!` need Shift on the keyboard but are bound without it.
        assert_eq!(
            lookup(Key::greater, ModifierType::SHIFT_MASK),
            Some("playlist-next")
        );
        assert_eq!(
            lookup(Key::exclam, ModifierType::SHIFT_MASK),
            Some("chapter-next-alt")
        );
        // Shift+E arrives as `E` with Shift; bound as `<Shift>e`.
        assert_eq!(
            lookup(Key::E, ModifierType::SHIFT_MASK),
            Some("cycle-edition")
        );
        assert_eq!(
            lookup(Key::Right, ModifierType::SHIFT_MASK),
            Some("seek-forward-long")
        );
        assert_eq!(
            lookup(Key::o, ModifierType::CONTROL_MASK),
            Some("open-file")
        );
    }

    #[test]
    fn every_listed_shortcut_has_a_target() {
        // Without a backend the mpv actions report unhandled, so tell them
        // apart from ids nothing dispatches by the list below.
        let (tx, _rx) = relm4::channel::<VideoShellInput>();
        let needs_backend = [
            "seek-forward",
            "seek-backward",
            "seek-forward-long",
            "seek-backward-long",
            "speed-up",
            "speed-down",
            "speed-reset",
            "chapter-next",
            "chapter-prev",
            "chapter-next-alt",
            "chapter-prev-alt",
            "frame-step",
            "frame-back-step",
            "revert-seek",
            "deinterlace",
            "hwdec",
            "panscan-up",
            "panscan-down",
            "volume-up",
            "volume-down",
            "mute",
            "audio-passthrough",
            "secondary-sub",
            "sub-seek-forward",
            "sub-seek-backward",
            "cycle-cc",
            "cycle-edition",
        ];
        for shortcut in crate::video::keybindings::SHORTCUTS {
            let dispatched =
                dispatch_video_shortcut(shortcut.action, &no_mpv(), &no_tv(), &tx, 150.0);
            assert!(
                dispatched || needs_backend.contains(&shortcut.action),
                "{} does nothing",
                shortcut.action
            );
        }
    }

    #[test]
    fn unknown_action_is_unhandled() {
        let (tx, _rx) = relm4::channel::<VideoShellInput>();
        assert!(!dispatch_video_shortcut(
            "does-not-exist",
            &no_mpv(),
            &no_tv(),
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
