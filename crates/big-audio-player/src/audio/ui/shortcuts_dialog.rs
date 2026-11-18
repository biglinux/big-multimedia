//! Keyboard-shortcuts window. The single-key event controller lives with the
//! shell's keybinding wiring, so only the viewer/rebind dialog is needed here.

use adw::prelude::*;
use big_app_kit::keybindings::ShortcutDef;
use big_relm4_components::input::shortcut_dialog::{
    BigShortcutCategory, BigShortcutDialog, BigShortcutDialogSpec, BigShortcutEntry,
    connect_shortcut_dialog_rebinds,
};

use crate::i18n::i18n;
use crate::settings::{Settings, keys};

/// Present the keyboard-shortcuts dialog transient to `parent`. Rebinds
/// persist to `CUSTOM_KEYBINDINGS`.
pub(crate) fn present_shortcuts(parent: &impl IsA<gtk::Window>, settings: &Settings) {
    use crate::audio::keybindings::{
        SHORTCUTS, display_accel, key_to_accel_string, resolve_accels,
    };

    let custom_str = settings.get_string(keys::CUSTOM_KEYBINDINGS);
    let resolved = resolve_accels(SHORTCUTS, &custom_str);

    let dialog = BigShortcutDialog::new(
        BigShortcutDialogSpec::new(i18n("Keyboard Shortcuts")),
        &shortcut_categories(),
        &shortcut_entries(&resolved, display_accel),
    );
    dialog.window().set_transient_for(Some(parent.as_ref()));

    let s = settings.clone();
    connect_shortcut_dialog_rebinds(
        &dialog,
        &custom_str,
        crate::shared::shortcut_capture::shortcut_capture_labels(),
        key_to_accel_string,
        display_accel,
        move |serialized| {
            let _ = s.put_string(keys::CUSTOM_KEYBINDINGS, &serialized);
        },
    );

    dialog.window().present();
}

fn shortcut_categories() -> Vec<BigShortcutCategory> {
    crate::audio::keybindings::CATEGORIES
        .iter()
        .map(|category| BigShortcutCategory::new(*category, i18n(category)))
        .collect()
}

fn shortcut_entries(
    resolved: &[(&ShortcutDef, String)],
    display_accel: fn(&str) -> String,
) -> Vec<BigShortcutEntry> {
    resolved
        .iter()
        .map(|(shortcut, accel)| {
            BigShortcutEntry::new(
                shortcut.category,
                i18n(shortcut.display_name),
                shortcut.action,
                shortcut.default_accel,
                display_accel(accel),
            )
        })
        .collect()
}
