//! bigagents: app-local-adapter; binds shortcut capture labels to app i18n.

use gtk::gdk;

use crate::i18n::i18n;

pub fn shortcut_capture_labels()
-> big_relm4_components::input::shortcut_capture::BigShortcutCaptureLabels {
    big_relm4_components::input::shortcut_capture::BigShortcutCaptureLabels::new(
        i18n("Set Shortcut"),
        i18n("Press the desired key combination…"),
        i18n("Waiting…"),
        i18n("Reset to Default"),
    )
}

pub fn show_shortcut_capture_dialog<FK, FF, FR, FA>(
    transient_for: &impl gtk::prelude::IsA<gtk::Window>,
    display_name: &str,
    _default_binding: &str,
    key_to_binding: FK,
    format_binding: FF,
    on_reset: FR,
    on_apply: FA,
) where
    FK: Fn(gdk::Key, gdk::ModifierType) -> String + 'static,
    FF: Fn(&str) -> String + 'static,
    FR: Fn() + 'static,
    FA: Fn(String) + 'static,
{
    big_relm4_components::input::shortcut_capture::show_shortcut_capture_dialog(
        transient_for,
        display_name,
        shortcut_capture_labels(),
        key_to_binding,
        format_binding,
        on_reset,
        on_apply,
    );
}
