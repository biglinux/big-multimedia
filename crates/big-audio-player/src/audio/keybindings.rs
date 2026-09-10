//! Audio-player keybinding table. Core types + helpers live in
//! `big-app-kit`; this module owns only the `SHORTCUTS` data.

pub use big_app_kit::keybindings::{
    ShortcutDef, build_accel_map, display_accel, key_to_accel_string, resolve_accels,
};

use big_media_runtime::i18n::gettext_noop;

pub const SHORTCUTS: &[ShortcutDef] = &[
    // ── Playback ──
    ShortcutDef {
        action: "play-pause",
        display_name: gettext_noop("Play / Pause"),
        category: gettext_noop("Playback"),
        default_accel: "space",
        single_key: true,
    },
    ShortcutDef {
        action: "next-track",
        display_name: gettext_noop("Next Track"),
        category: gettext_noop("Playback"),
        default_accel: "<Ctrl>Right",
        single_key: false,
    },
    ShortcutDef {
        action: "prev-track",
        display_name: gettext_noop("Previous Track"),
        category: gettext_noop("Playback"),
        default_accel: "<Ctrl>Left",
        single_key: false,
    },
    ShortcutDef {
        action: "next-track",
        display_name: gettext_noop("Next Track (single key)"),
        category: gettext_noop("Playback"),
        default_accel: "n",
        single_key: true,
    },
    ShortcutDef {
        action: "prev-track",
        display_name: gettext_noop("Previous Track (single key)"),
        category: gettext_noop("Playback"),
        default_accel: "p",
        single_key: true,
    },
    // ── Navigation ──
    ShortcutDef {
        action: "search",
        display_name: gettext_noop("Search"),
        category: gettext_noop("Navigation"),
        default_accel: "<Ctrl>f",
        single_key: false,
    },
    ShortcutDef {
        action: "scroll-to-playing",
        display_name: gettext_noop("Scroll to Playing"),
        category: gettext_noop("Navigation"),
        default_accel: "<Ctrl>j",
        single_key: false,
    },
    ShortcutDef {
        action: "select-all",
        display_name: gettext_noop("Select All"),
        category: gettext_noop("Navigation"),
        default_accel: "<Ctrl>a",
        single_key: false,
    },
    // ── Library ──
    ShortcutDef {
        action: "add-folder",
        display_name: gettext_noop("Manage Music Folders"),
        category: gettext_noop("Library"),
        default_accel: "<Ctrl>o",
        single_key: false,
    },
    ShortcutDef {
        action: "show-equalizer",
        display_name: gettext_noop("Equalizer"),
        category: gettext_noop("Library"),
        default_accel: "<Ctrl>e",
        single_key: false,
    },
    // ── Queue ──
    ShortcutDef {
        action: "toggle-shuffle",
        display_name: gettext_noop("Toggle Shuffle"),
        category: gettext_noop("Queue"),
        default_accel: "<Ctrl>s",
        single_key: false,
    },
    ShortcutDef {
        action: "toggle-repeat",
        display_name: gettext_noop("Toggle Repeat"),
        category: gettext_noop("Queue"),
        default_accel: "<Ctrl>r",
        single_key: false,
    },
    ShortcutDef {
        action: "undo",
        display_name: gettext_noop("Undo"),
        category: gettext_noop("Queue"),
        default_accel: "<Ctrl>z",
        single_key: false,
    },
    ShortcutDef {
        action: "redo",
        display_name: gettext_noop("Redo"),
        category: gettext_noop("Queue"),
        default_accel: "<Ctrl><Shift>z",
        single_key: false,
    },
    ShortcutDef {
        action: "app.quit",
        display_name: gettext_noop("Quit"),
        category: gettext_noop("Queue"),
        default_accel: "<Ctrl>q",
        single_key: false,
    },
];

/// Display order in the shortcuts dialog.
pub const CATEGORIES: &[&str] = &["Playback", "Navigation", "Library", "Queue"];

#[must_use]
pub fn build_single_key_map(custom: &str) -> std::collections::HashMap<String, String> {
    build_accel_map(SHORTCUTS, custom, true)
}
