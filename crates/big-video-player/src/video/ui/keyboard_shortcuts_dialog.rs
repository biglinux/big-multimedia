//! App-local keyboard-shortcuts dialog launcher (video port, V5).
//!
//! Thin glue over the shared `big_relm4_components` shortcut dialog +
//! `big_app_kit::keybindings` rebind plumbing: builds the categorized shortcut
//! list from the video keybinding table, presents it transient to the player
//! window, and persists click-to-rebind edits to `Settings`. Reuses the shared
//! dialog/capture widgets rather than re-implementing them; only the video
//! shortcut table + persistence are app-local.

use adw::prelude::*;
use big_app_kit::keybindings::ShortcutDef;
use big_relm4_components::input::shortcut_dialog::{
    connect_shortcut_dialog_rebinds, BigShortcutCategory, BigShortcutDialog, BigShortcutDialogSpec,
    BigShortcutEntry,
};

use crate::i18n::i18n;
use crate::settings::{keys, Settings};
use crate::video::keybindings::{
    display_accel, key_to_accel_string, resolve_accels, CATEGORIES, SHORTCUTS,
};

/// Present the keyboard-shortcuts dialog transient to `parent`, wiring
/// click-to-rebind persistence into `settings`.
pub fn show_video_keyboard_shortcuts_dialog(parent: &impl IsA<gtk::Window>, settings: &Settings) {
    let custom = settings.get_string(keys::CUSTOM_KEYBINDINGS);
    let resolved = resolve_accels(SHORTCUTS, &custom);

    let dialog = BigShortcutDialog::new(
        BigShortcutDialogSpec::new(i18n("Keyboard Shortcuts")),
        &shortcut_categories(),
        &shortcut_entries(&resolved),
    );
    dialog.window().set_transient_for(Some(parent));

    let settings = settings.clone();
    connect_shortcut_dialog_rebinds(
        &dialog,
        dialog.window(),
        &custom,
        crate::shared::shortcut_capture::shortcut_capture_labels(),
        key_to_accel_string,
        display_accel,
        move |serialized| {
            settings.put_string(keys::CUSTOM_KEYBINDINGS, &serialized);
        },
    );

    dialog.window().present();
}

fn shortcut_categories() -> Vec<BigShortcutCategory> {
    CATEGORIES
        .iter()
        .map(|category| {
            BigShortcutCategory::new(*category, i18n(category))
                .description(category_description(category))
        })
        .collect()
}

fn category_description(category: &str) -> String {
    match category {
        "Playback" => i18n("Play, pause, seek and navigate"),
        "Navigation" => i18n("Chapter, edition and recent files"),
        "Video" => i18n("Fullscreen, crop, aspect ratio"),
        "Audio" => i18n("Volume and audio track switching"),
        "Subtitles" => i18n("Subtitle track selection and timing"),
        "Screenshots" => i18n("Capture still frames"),
        "Music" => i18n("Music-mode specific controls"),
        _ => i18n("Miscellaneous shortcuts"),
    }
}

fn shortcut_entries(resolved: &[(&ShortcutDef, String)]) -> Vec<BigShortcutEntry> {
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
