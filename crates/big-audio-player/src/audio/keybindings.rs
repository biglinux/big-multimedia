//! Audio-player keybinding table. Core types + helpers live in
//! `big-app-kit`; this module owns only the `SHORTCUTS` data.

pub use big_app_kit::keybindings::{
    build_accel_map, display_accel, key_to_accel_string, resolve_accels, ShortcutDef,
};

pub const SHORTCUTS: &[ShortcutDef] = &[
    // ── Playback ──
    ShortcutDef {
        action: "play-pause",
        display_name: "Play / Pause",
        category: "Playback",
        default_accel: "space",
        single_key: true,
    },
    ShortcutDef {
        action: "next-track",
        display_name: "Next Track",
        category: "Playback",
        default_accel: "<Ctrl>Right",
        single_key: false,
    },
    ShortcutDef {
        action: "prev-track",
        display_name: "Previous Track",
        category: "Playback",
        default_accel: "<Ctrl>Left",
        single_key: false,
    },
    ShortcutDef {
        action: "next-track",
        display_name: "Next Track (single key)",
        category: "Playback",
        default_accel: "n",
        single_key: true,
    },
    ShortcutDef {
        action: "prev-track",
        display_name: "Previous Track (single key)",
        category: "Playback",
        default_accel: "p",
        single_key: true,
    },
    // ── Navigation ──
    ShortcutDef {
        action: "search",
        display_name: "Search",
        category: "Navigation",
        default_accel: "<Ctrl>f",
        single_key: false,
    },
    ShortcutDef {
        action: "scroll-to-playing",
        display_name: "Scroll to Playing",
        category: "Navigation",
        default_accel: "<Ctrl>j",
        single_key: false,
    },
    ShortcutDef {
        action: "select-all",
        display_name: "Select All",
        category: "Navigation",
        default_accel: "<Ctrl>a",
        single_key: false,
    },
    // ── Library ──
    ShortcutDef {
        action: "add-folder",
        display_name: "Manage Music Folders",
        category: "Library",
        default_accel: "<Ctrl>o",
        single_key: false,
    },
    ShortcutDef {
        action: "show-equalizer",
        display_name: "Equalizer",
        category: "Library",
        default_accel: "<Ctrl>e",
        single_key: false,
    },
    // ── Queue ──
    ShortcutDef {
        action: "toggle-shuffle",
        display_name: "Toggle Shuffle",
        category: "Queue",
        default_accel: "<Ctrl>s",
        single_key: false,
    },
    ShortcutDef {
        action: "toggle-repeat",
        display_name: "Toggle Repeat",
        category: "Queue",
        default_accel: "<Ctrl>r",
        single_key: false,
    },
    ShortcutDef {
        action: "undo",
        display_name: "Undo",
        category: "Queue",
        default_accel: "<Ctrl>z",
        single_key: false,
    },
    ShortcutDef {
        action: "redo",
        display_name: "Redo",
        category: "Queue",
        default_accel: "<Ctrl><Shift>z",
        single_key: false,
    },
    ShortcutDef {
        action: "app.quit",
        display_name: "Quit",
        category: "Queue",
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
