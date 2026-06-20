//! Video-player keybinding table. Core helpers live in `big-app-kit`.

pub use big_app_kit::keybindings::{
    build_accel_map, display_accel, key_to_accel_string, resolve_accels, ShortcutDef,
};

/// `single_key: true` everywhere — dispatch via `EventControllerKey` in `window_input.rs`,
/// not GTK's `set_accels_for_action`. Same-accel duplicates collapse in `build_accel_map`.
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
        action: "seek-forward",
        display_name: "Seek forward 5 s",
        category: "Playback",
        default_accel: "Right",
        single_key: true,
    },
    ShortcutDef {
        action: "seek-backward",
        display_name: "Seek backward 5 s",
        category: "Playback",
        default_accel: "Left",
        single_key: true,
    },
    ShortcutDef {
        action: "seek-forward-long",
        display_name: "Seek forward 30 s",
        category: "Playback",
        default_accel: "<Shift>Right",
        single_key: true,
    },
    ShortcutDef {
        action: "seek-backward-long",
        display_name: "Seek backward 30 s",
        category: "Playback",
        default_accel: "<Shift>Left",
        single_key: true,
    },
    ShortcutDef {
        action: "speed-up",
        display_name: "Increase speed",
        category: "Playback",
        default_accel: "bracketright",
        single_key: true,
    },
    ShortcutDef {
        action: "speed-down",
        display_name: "Decrease speed",
        category: "Playback",
        default_accel: "bracketleft",
        single_key: true,
    },
    ShortcutDef {
        action: "speed-reset",
        display_name: "Reset speed to 1×",
        category: "Playback",
        default_accel: "BackSpace",
        single_key: true,
    },
    ShortcutDef {
        action: "chapter-next",
        display_name: "Next chapter",
        category: "Playback",
        default_accel: "Page_Up",
        single_key: true,
    },
    ShortcutDef {
        action: "chapter-prev",
        display_name: "Previous chapter",
        category: "Playback",
        default_accel: "Page_Down",
        single_key: true,
    },
    ShortcutDef {
        action: "playlist-next",
        display_name: "Next in playlist",
        category: "Playback",
        default_accel: "greater",
        single_key: true,
    },
    ShortcutDef {
        action: "playlist-prev",
        display_name: "Previous in playlist",
        category: "Playback",
        default_accel: "less",
        single_key: true,
    },
    ShortcutDef {
        action: "frame-step",
        display_name: "Next frame",
        category: "Playback",
        default_accel: "period",
        single_key: true,
    },
    ShortcutDef {
        action: "frame-back-step",
        display_name: "Previous frame",
        category: "Playback",
        default_accel: "comma",
        single_key: true,
    },
    ShortcutDef {
        action: "revert-seek",
        display_name: "Undo last seek",
        category: "Playback",
        default_accel: "<Shift>BackSpace",
        single_key: true,
    },
    // ── Video ──
    ShortcutDef {
        action: "fullscreen",
        display_name: "Toggle fullscreen",
        category: "Video",
        default_accel: "f",
        single_key: true,
    },
    ShortcutDef {
        action: "fullscreen-f11",
        display_name: "Toggle fullscreen (F11)",
        category: "Video",
        default_accel: "F11",
        single_key: true,
    },
    ShortcutDef {
        action: "exit-fullscreen",
        display_name: "Exit fullscreen",
        category: "Video",
        default_accel: "Escape",
        single_key: true,
    },
    ShortcutDef {
        action: "deinterlace",
        display_name: "Toggle deinterlace",
        category: "Video",
        default_accel: "d",
        single_key: true,
    },
    ShortcutDef {
        action: "hwdec",
        display_name: "Toggle hardware decoding",
        category: "Video",
        default_accel: "<Ctrl>h",
        single_key: true,
    },
    ShortcutDef {
        action: "panscan-up",
        display_name: "Increase panscan",
        category: "Video",
        default_accel: "e",
        single_key: true,
    },
    ShortcutDef {
        action: "panscan-down",
        display_name: "Decrease panscan",
        category: "Video",
        default_accel: "w",
        single_key: true,
    },
    ShortcutDef {
        action: "crop",
        display_name: "Crop video",
        category: "Video",
        default_accel: "<Ctrl>c",
        single_key: true,
    },
    // ── Audio ──
    ShortcutDef {
        action: "volume-up",
        display_name: "Volume up",
        category: "Audio",
        default_accel: "Up",
        single_key: true,
    },
    ShortcutDef {
        action: "volume-down",
        display_name: "Volume down",
        category: "Audio",
        default_accel: "Down",
        single_key: true,
    },
    ShortcutDef {
        action: "mute",
        display_name: "Toggle mute",
        category: "Audio",
        default_accel: "m",
        single_key: true,
    },
    ShortcutDef {
        action: "audio-passthrough",
        display_name: "Toggle audio passthrough",
        category: "Audio",
        default_accel: "<Ctrl>p",
        single_key: true,
    },
    // ── Subtitles ──
    ShortcutDef {
        action: "secondary-sub",
        display_name: "Toggle secondary subtitles",
        category: "Subtitles",
        default_accel: "v",
        single_key: true,
    },
    ShortcutDef {
        action: "sub-seek-forward",
        display_name: "Next subtitle",
        category: "Subtitles",
        default_accel: "<Ctrl>Right",
        single_key: true,
    },
    ShortcutDef {
        action: "sub-seek-backward",
        display_name: "Previous subtitle",
        category: "Subtitles",
        default_accel: "<Ctrl>Left",
        single_key: true,
    },
    ShortcutDef {
        action: "cycle-cc",
        display_name: "Cycle Closed Captions",
        category: "Subtitles",
        default_accel: "c",
        single_key: true,
    },
    // ── Screenshots ──
    ShortcutDef {
        action: "screenshot",
        display_name: "Screenshot",
        category: "Screenshots",
        default_accel: "s",
        single_key: true,
    },
    // ── Music ──
    ShortcutDef {
        action: "music-shuffle",
        display_name: "Toggle shuffle",
        category: "Music",
        default_accel: "z",
        single_key: true,
    },
    ShortcutDef {
        action: "music-repeat",
        display_name: "Cycle repeat mode",
        category: "Music",
        default_accel: "r",
        single_key: true,
    },
    ShortcutDef {
        action: "music-replaygain",
        display_name: "Cycle ReplayGain",
        category: "Music",
        default_accel: "<Ctrl>r",
        single_key: true,
    },
    ShortcutDef {
        action: "music-gapless",
        display_name: "Toggle gapless",
        category: "Music",
        default_accel: "g",
        single_key: true,
    },
    // ── Navigation ──
    ShortcutDef {
        action: "chapter-next",
        display_name: "Next chapter",
        category: "Navigation",
        default_accel: "exclam",
        single_key: true,
    },
    ShortcutDef {
        action: "chapter-prev",
        display_name: "Previous chapter",
        category: "Navigation",
        default_accel: "at",
        single_key: true,
    },
    // ── Other ──
    ShortcutDef {
        action: "info-overlay",
        display_name: "Toggle file info overlay",
        category: "Other",
        default_accel: "i",
        single_key: true,
    },
    ShortcutDef {
        action: "open-file",
        display_name: "Open file",
        category: "Other",
        default_accel: "<Ctrl>o",
        single_key: true,
    },
    ShortcutDef {
        action: "open-url",
        display_name: "Open URL",
        category: "Other",
        default_accel: "<Ctrl>u",
        single_key: true,
    },
    ShortcutDef {
        action: "paste-play",
        display_name: "Paste and play",
        category: "Other",
        default_accel: "<Ctrl>v",
        single_key: true,
    },
    ShortcutDef {
        action: "goto-time",
        display_name: "Go to time",
        category: "Other",
        default_accel: "<Ctrl>t",
        single_key: true,
    },
    ShortcutDef {
        action: "stream-record",
        display_name: "Toggle stream recording",
        category: "Other",
        // Was <Ctrl>r → clashed with music-replaygain.
        default_accel: "<Ctrl><Shift>r",
        single_key: true,
    },
    ShortcutDef {
        action: "cycle-edition",
        display_name: "Cycle edition",
        category: "Other",
        // Was `e` → clashed with panscan-up. mpv default is Shift+E.
        default_accel: "<Shift>e",
        single_key: true,
    },
    ShortcutDef {
        action: "media-info",
        display_name: "Media information",
        category: "Other",
        default_accel: "<Ctrl>i",
        single_key: true,
    },
];

/// Display order. Categories absent from this list silently drop their shortcuts from the dialog.
pub const CATEGORIES: &[&str] = &[
    "Playback",
    "Video",
    "Audio",
    "Subtitles",
    "Screenshots",
    "Music",
    "Navigation",
    "Other",
];

#[must_use]
pub fn build_key_map(custom: &str) -> std::collections::HashMap<String, String> {
    build_accel_map(SHORTCUTS, custom, false)
}

pub use display_accel as display_key;

// Legacy alias — kept so older `key_to_string(...)` call sites still resolve.
pub use key_to_accel_string as key_to_string;
