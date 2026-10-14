// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! GTK accelerator parsing, migration, and shortcut override helpers.
//!
//! The display-free shortcut specs and override parsing (`ShortcutDef`,
//! `resolve_accels`, `parse_custom`, …) live in
//! `big_app_kit_core::keybindings` and are re-exported below; the gdk keyval
//! helpers and the GTK-backed `display_accel` label formatter stay here.

use gtk::gdk;
use gtk::prelude::IsA;

#[doc(inline)]
pub use big_app_kit_core::keybindings::{
    ShortcutDef, build_accel_map, parse_custom, remove_custom_override, resolve_accels,
    serialize_custom, set_custom_override,
};

/// Format a `(keyval, modifiers)` pair as the GTK accelerator string.
///
/// Modifier mask bits are emitted as the canonical `<Ctrl>`/`<Shift>`/`<Alt>`
/// prefixes (in that order) followed by the GDK key name. Use the result
/// with [`gtk::accelerator_parse`] or shortcut controllers.
#[must_use]
pub fn key_to_accel_string(keyval: gdk::Key, state: gdk::ModifierType) -> String {
    let mut parts = String::new();
    if state.contains(gdk::ModifierType::CONTROL_MASK) {
        parts.push_str("<Ctrl>");
    }
    if state.contains(gdk::ModifierType::SHIFT_MASK) {
        parts.push_str("<Shift>");
    }
    if state.contains(gdk::ModifierType::ALT_MASK) {
        parts.push_str("<Alt>");
    }
    parts.push_str(&key_name_for_accel_string(keyval));
    parts
}

/// Find the action bound to a key press in an accel → action `map`.
///
/// The press is tried as typed, then lower-cased (Caps Lock gives `F` for
/// `f`, Shift+E gives `<Shift>E` for `<Shift>e`), then without a Shift the
/// symbol already carries (`>` arrives as Shift+`greater` but is bound as
/// `greater`). Modifiers other than Ctrl, Shift and Alt are ignored.
#[must_use]
pub fn lookup_shortcut<S: std::hash::BuildHasher>(
    map: &std::collections::HashMap<String, String, S>,
    keyval: gdk::Key,
    state: gdk::ModifierType,
) -> Option<&str> {
    use gdk::ModifierType;
    let state =
        state & (ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK | ModifierType::ALT_MASK);
    let mut candidates = vec![
        key_to_accel_string(keyval, state),
        key_to_accel_string(keyval.to_lower(), state),
    ];
    if state.contains(ModifierType::SHIFT_MASK) {
        candidates.push(key_to_accel_string(
            keyval,
            state - ModifierType::SHIFT_MASK,
        ));
    }
    candidates
        .iter()
        .find_map(|accel| map.get(accel.as_str()))
        .map(String::as_str)
}

/// Whether the focus of the window holding `widget` is a text editor, where
/// bare-key shortcuts must yield so typing works.
#[must_use]
pub fn focus_accepts_text(widget: &gtk::Widget) -> bool {
    use gtk::prelude::*;
    let Some(focus) = widget
        .root()
        .and_downcast::<gtk::Window>()
        .and_then(|window| gtk::prelude::GtkWindowExt::focus(&window))
    else {
        return false;
    };
    focus.dynamic_cast_ref::<gtk::Editable>().is_some()
        || focus.is::<gtk::TextView>()
        || focus.ancestor(gtk::TextView::static_type()).is_some()
}

/// Keep `controller` on the window that hosts `content` while `content` is
/// realized, and take it off on unrealize, so a window that outlives the
/// content (a host remounting it) keeps no handler that owns its state.
///
/// Controllers on the content see a key only when the focus is inside it.
/// With no focus widget (a click on a surface that takes no focus, or the
/// focused button hidden away) the key reaches the window alone, so a
/// capture-phase controller attached here is what still hears it.
pub fn attach_to_window(content: &impl IsA<gtk::Widget>, controller: gtk::EventControllerKey) {
    use gtk::prelude::*;
    let attached: std::rc::Rc<std::cell::RefCell<Option<gtk::glib::WeakRef<gtk::Window>>>> =
        std::rc::Rc::default();
    content.as_ref().connect_realize({
        let attached = attached.clone();
        let controller = controller.clone();
        move |content| {
            let Some(window) = content.root().and_downcast::<gtk::Window>() else {
                return;
            };
            if controller.widget().is_none() {
                window.add_controller(controller.clone());
                attached.replace(Some(window.downgrade()));
            }
        }
    });
    content.as_ref().connect_unrealize(move |_| {
        if let Some(window) = attached.take().and_then(|window| window.upgrade()) {
            window.remove_controller(&controller);
        }
    });
}

fn key_name_for_accel_string(keyval: gdk::Key) -> String {
    #[cfg(miri)]
    {
        use gtk::glib::translate::IntoGlib;

        miri_key_name_from_raw_keyval(keyval.into_glib(), gdk::Key::space.into_glib())
    }

    #[cfg(not(miri))]
    {
        keyval
            .name()
            .map_or_else(String::new, |name| name.to_string())
    }
}

#[cfg(any(test, miri))]
fn miri_key_name_from_raw_keyval(raw_keyval: u32, space_keyval: u32) -> String {
    if raw_keyval == space_keyval {
        return "space".to_string();
    }
    if (0x21..=0x7e).contains(&raw_keyval)
        && let Some(character) = char::from_u32(raw_keyval)
    {
        return character.to_string();
    }
    String::new()
}

/// Convert a GTK accelerator string into a user-facing label.
///
/// Uses [`gtk::accelerator_parse`] + [`gtk::accelerator_get_label`] when GTK is
/// initialized on the main thread. In headless contexts or on parse failure,
/// applies BigLinux substitutions (`space` -> `Space`, `bracketleft` and
/// `bracketright` -> `[`/`]`, `Page_Up` -> `Page Up`, `BackSpace` ->
/// `Backspace`) so shortcut dialogs and tests never render raw keysyms.
#[must_use]
pub fn display_accel(accel: &str) -> String {
    if gtk::is_initialized_main_thread()
        && let Some((key, mods)) = gtk::accelerator_parse(accel)
    {
        return gtk::accelerator_get_label(key, mods).to_string();
    }

    fallback_display_accel(accel)
}

fn fallback_display_accel(accel: &str) -> String {
    accel
        .replace("space", "Space")
        .replace("bracketright", "]")
        .replace("bracketleft", "[")
        .replace("Page_Up", "Page Up")
        .replace("Page_Down", "Page Down")
        .replace("BackSpace", "Backspace")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accel_string_orders_modifiers() {
        let state = gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::ALT_MASK;

        assert_eq!(
            key_to_accel_string(gdk::Key::m, state),
            "<Ctrl><Shift><Alt>m"
        );
    }

    #[test]
    fn key_name_fallback_covers_ascii() {
        use gtk::glib::translate::IntoGlib;

        let space_keyval = gdk::Key::space.into_glib();

        assert_eq!(
            miri_key_name_from_raw_keyval(gdk::Key::m.into_glib(), space_keyval),
            "m"
        );
        assert_eq!(
            miri_key_name_from_raw_keyval(gdk::Key::_1.into_glib(), space_keyval),
            "1"
        );
        assert_eq!(
            miri_key_name_from_raw_keyval(gdk::Key::space.into_glib(), space_keyval),
            "space"
        );
        assert_eq!(
            miri_key_name_from_raw_keyval(gdk::Key::F1.into_glib(), space_keyval),
            ""
        );
    }

    #[test]
    fn display_accel_uses_readable_key_labels() {
        assert_eq!(display_accel("space"), "Space");
        assert_eq!(display_accel("bracketleft"), "[");
        assert_eq!(display_accel("Page_Down"), "Page Down");
    }
}
