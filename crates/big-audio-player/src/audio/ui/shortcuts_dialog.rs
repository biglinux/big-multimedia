//! Keyboard-shortcuts window. Restored app-local from the god-object's
//! `present_shortcuts_window`; the single-key event controller lives with the
//! shell's keybinding wiring, so only the viewer/rebind dialog is needed here.

use adw::prelude::*;
use big_app_kit::keybindings::{connect_shortcut_dialog_rebinds, ShortcutDef};
use big_relm4_components::input::shortcut_dialog::{
    BigShortcutCategory, BigShortcutDialog, BigShortcutDialogSpec, BigShortcutEntry,
};

use crate::i18n::i18n;
use crate::settings::{keys, Settings};

/// Present the keyboard-shortcuts dialog. Rebinds persist to
/// `CUSTOM_KEYBINDINGS`; `parent` is the transient/capture parent.
pub(crate) fn present_shortcuts(parent: &impl IsA<gtk::Window>, settings: &Settings) {
    use crate::audio::keybindings::{
        display_accel, key_to_accel_string, resolve_accels, SHORTCUTS,
    };

    let custom_str = settings.get_string(keys::CUSTOM_KEYBINDINGS);
    // Vec preserves duplicate actions (single-key + multi-key defs); indexing by
    // the exact def gives the right accel for each row.
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
        parent,
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
