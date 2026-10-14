//! Keyboard shortcuts of the audio player.
//!
//! [`crate::audio::keybindings::SHORTCUTS`] is the catalog the shortcuts
//! dialog shows and rebinds; this module makes every entry act. The `win.*`
//! actions live on the content, not the window, so GTK application
//! accelerators never reached them: the controllers here dispatch the modified
//! bindings as well as the bare keys.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::*;

use big_app_kit::keybindings::{attach_to_window, focus_accepts_text, lookup_shortcut};

use crate::audio::keybindings::SHORTCUTS;
use crate::audio::ui::library_grouping_ui::LIBRARY_FILTER_NAME;

/// Install the shortcut controllers.
///
/// Controllers on the content root see a key only when the focus is inside
/// it; with nothing focused the key reaches the window alone, and the
/// window's own bindings eat Space and the arrows. A capture-phase controller
/// on the window handles that case.
pub(super) fn wire_keyboard_shortcuts(
    root: &gtk::Box,
    settings: &Settings,
    sender: &ComponentSender<AudioShell>,
) {
    let shortcuts = Rc::new(Shortcuts {
        bindings: RefCell::default(),
        settings: settings.clone(),
        input: sender.input_sender().clone(),
        root: root.downgrade(),
    });

    // Taken on the way down, before the focused widget: a list uses Space
    // to activate its row and the arrows to move, a button presses itself on
    // Space. Text, sliders and the library tree keep their arrows, text and
    // menus keep Space.
    let transport = gtk::EventControllerKey::new();
    transport.set_propagation_phase(gtk::PropagationPhase::Capture);
    transport.connect_key_pressed({
        let shortcuts = shortcuts.clone();
        move |controller, keyval, _, state| {
            let Some(widget) = controller.widget() else {
                return glib::Propagation::Proceed;
            };
            let Some(action) = shortcuts.action_for(keyval, state) else {
                return glib::Propagation::Proceed;
            };
            // Seeking and changing track ride on the arrows (Ctrl+arrow moves
            // the focus in a list).
            let takes_it = if action.starts_with("seek-") || action.ends_with("-track") {
                !focus_keeps_arrows(&widget)
            } else if action == "play-pause" {
                !focus_accepts_text(&widget) && !focus_in_menu(&widget)
            } else {
                false
            };
            if takes_it {
                shortcuts.dispatch(&action)
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    root.add_controller(transport);

    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let shortcuts = shortcuts.clone();
        move |controller, keyval, _, state| {
            let in_text = controller.widget().is_some_and(|w| focus_accepts_text(&w));
            shortcuts.handle(keyval, state, in_text)
        }
    });
    root.add_controller(keys);

    let window_keys = gtk::EventControllerKey::new();
    window_keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    window_keys.connect_key_pressed(move |controller, keyval, _, state| {
        let Some(window) = controller.widget().and_downcast::<gtk::Window>() else {
            return glib::Propagation::Proceed;
        };
        if gtk::prelude::GtkWindowExt::focus(&window).is_some() {
            return glib::Propagation::Proceed;
        }
        shortcuts.handle(keyval, state, false)
    });
    attach_to_window(root, window_keys);
}

/// What the shortcut keys act on.
struct Shortcuts {
    /// The saved custom bindings and the key map built from them, rebuilt
    /// when the shortcuts dialog saves a change.
    bindings: RefCell<(Option<String>, HashMap<String, String>)>,
    settings: Settings,
    input: relm4::Sender<AudioShellInput>,
    root: glib::WeakRef<gtk::Box>,
}

impl Shortcuts {
    /// The action bound to a key press under the current bindings.
    fn action_for(&self, keyval: gdk::Key, state: gdk::ModifierType) -> Option<String> {
        let custom = self.settings.get_string(keys::CUSTOM_KEYBINDINGS);
        let mut bindings = self.bindings.borrow_mut();
        if bindings.0.as_deref() != Some(custom.as_str()) {
            *bindings = (
                Some(custom.clone()),
                big_app_kit::keybindings::build_accel_map(SHORTCUTS, &custom, false),
            );
        }
        lookup_shortcut(&bindings.1, keyval, state).map(str::to_string)
    }

    fn handle(
        &self,
        keyval: gdk::Key,
        state: gdk::ModifierType,
        in_text: bool,
    ) -> glib::Propagation {
        // Hardware media keys act regardless of focus.
        if let Some(action) = media_key_action(keyval) {
            return self.dispatch(action);
        }
        let ctrl_y =
            keyval.to_lower() == gdk::Key::y && state.contains(gdk::ModifierType::CONTROL_MASK);
        match self.action_for(keyval, state) {
            // A text field keeps the keys it edits with: typing, Ctrl+Z,
            // Ctrl+A, Ctrl+arrows. Ctrl+F, Ctrl+E and the like mean nothing
            // there and still act.
            Some(action) if in_text && !acts_from_text(&action) => glib::Propagation::Proceed,
            None if in_text => glib::Propagation::Proceed,
            Some(action) => self.dispatch(&action),
            // Ctrl+Y redoes too, as in most editors.
            None if ctrl_y => self.dispatch("redo"),
            None => glib::Propagation::Proceed,
        }
    }

    /// Act on `action`; keys with no target here keep propagating, so
    /// `app.quit` still reaches its application accelerator.
    fn dispatch(&self, action: &str) -> glib::Propagation {
        if action == "search" {
            return self.focus_filter();
        }
        let Some(message) = shortcut_message(action) else {
            return glib::Propagation::Proceed;
        };
        let _ = self.input.send(message);
        glib::Propagation::Stop
    }

    /// Ctrl+F: put the cursor in the library filter, when it is on screen.
    fn focus_filter(&self) -> glib::Propagation {
        let filter = self
            .root
            .upgrade()
            .and_then(|root| find_named(root.upcast_ref(), LIBRARY_FILTER_NAME))
            .filter(gtk::Widget::is_mapped);
        match filter {
            Some(filter) => {
                filter.grab_focus();
                glib::Propagation::Stop
            }
            None => glib::Propagation::Proceed,
        }
    }
}

/// The shell message a shortcut action sends; `None` for actions handled
/// elsewhere (`search` focuses a widget, `app.quit` is the application's).
/// The `win.*` actions of the main menu go through here as well.
pub(super) fn shortcut_message(action: &str) -> Option<AudioShellInput> {
    Some(match action {
        "play-pause" => AudioShellInput::Playback(AudioPlaybackBarOutput::TogglePlay),
        "seek-forward" => AudioShellInput::SeekBy(5.0),
        "seek-backward" => AudioShellInput::SeekBy(-5.0),
        "seek-forward-long" => AudioShellInput::SeekBy(30.0),
        "seek-backward-long" => AudioShellInput::SeekBy(-30.0),
        "next-track" => AudioShellInput::Playback(AudioPlaybackBarOutput::Next),
        "prev-track" => AudioShellInput::Playback(AudioPlaybackBarOutput::Previous),
        "toggle-repeat" => AudioShellInput::Playback(AudioPlaybackBarOutput::CycleRepeat),
        "toggle-shuffle" => AudioShellInput::ToggleShuffleShortcut,
        "undo" => AudioShellInput::Undo,
        "redo" => AudioShellInput::Redo,
        "scroll-to-playing" => AudioShellInput::ScrollToPlaying,
        "select-all" => AudioShellInput::SelectAllInQueue,
        "add-folder" => AudioShellInput::ShowManageFolders,
        "show-equalizer" => AudioShellInput::ShowEqualizer,
        _ => return None,
    })
}

/// Actions whose keys a text field has no use for, so they act from one.
fn acts_from_text(action: &str) -> bool {
    matches!(
        action,
        "search"
            | "scroll-to-playing"
            | "add-folder"
            | "show-equalizer"
            | "toggle-shuffle"
            | "toggle-repeat"
    )
}

/// Whether the focus is inside a menu (a popover), whose rows Space activates.
fn focus_in_menu(widget: &gtk::Widget) -> bool {
    widget
        .root()
        .and_downcast::<gtk::Window>()
        .and_then(|window| gtk::prelude::GtkWindowExt::focus(&window))
        .is_some_and(|focus| focus.ancestor(gtk::Popover::static_type()).is_some())
}

/// The first descendant of `widget` (or itself) with this widget name.
fn find_named(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if widget.widget_name() == name {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find_named(&current, name) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

fn media_key_action(keyval: gdk::Key) -> Option<&'static str> {
    match keyval {
        gdk::Key::AudioPlay | gdk::Key::AudioPause | gdk::Key::AudioMedia => Some("play-pause"),
        gdk::Key::AudioNext => Some("next-track"),
        gdk::Key::AudioPrev => Some("prev-track"),
        _ => None,
    }
}

/// Whether the focused widget uses Left/Right itself: text, a slider (the seek
/// bar and volume move with the arrows) or a library tree row.
fn focus_keeps_arrows(widget: &gtk::Widget) -> bool {
    if focus_accepts_text(widget) {
        return true;
    }
    let focus = widget
        .root()
        .and_downcast::<gtk::Window>()
        .and_then(|window| gtk::prelude::GtkWindowExt::focus(&window));
    focus.is_some_and(|focus| {
        focus.is::<gtk::Range>()
            || focus.ancestor(gtk::Range::static_type()).is_some()
            // The library tree opens and closes rows with them.
            || focus
                .downcast_ref::<big_item_view::ItemView>()
                .and_then(big_item_view::ItemView::model)
                .and_then(|model| model.item(0))
                .is_some_and(|item| item.is::<gtk::TreeListRow>())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::keybindings::build_single_key_map;

    #[test]
    fn every_listed_shortcut_has_a_target() {
        for shortcut in SHORTCUTS {
            // Search focuses the filter; quit is the application's.
            if matches!(shortcut.action, "search" | "app.quit") {
                continue;
            }
            assert!(
                shortcut_message(shortcut.action).is_some(),
                "{} does nothing",
                shortcut.action
            );
        }
    }

    #[test]
    fn bare_keys_match_through_caps_lock_and_shift() {
        let map = big_app_kit::keybindings::build_accel_map(SHORTCUTS, "", false);
        let lookup = |key, state| lookup_shortcut(&map, key, state);
        assert_eq!(
            lookup(gdk::Key::space, gdk::ModifierType::empty()),
            Some("play-pause")
        );
        assert_eq!(
            lookup(gdk::Key::N, gdk::ModifierType::LOCK_MASK),
            Some("next-track")
        );
        assert_eq!(
            lookup(gdk::Key::Right, gdk::ModifierType::CONTROL_MASK),
            Some("next-track")
        );
        assert_eq!(
            lookup(gdk::Key::f, gdk::ModifierType::CONTROL_MASK),
            Some("search")
        );
        assert_eq!(
            lookup(
                gdk::Key::Z,
                gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK
            ),
            Some("redo")
        );
        assert!(build_single_key_map("").contains_key("space"));
    }
}
