//! Video-player keybinding table. Core helpers live in `big-app-kit`.

pub use big_app_kit::keybindings::{
    ShortcutDef, build_accel_map, display_accel, key_to_accel_string, resolve_accels,
};

use big_media_runtime::i18n::gettext_noop;

/// `single_key: true` everywhere: dispatch via `EventControllerKey` in `window_input.rs`,
/// not GTK's `set_accels_for_action`. Same-accel duplicates collapse in `build_accel_map`.
pub const SHORTCUTS: &[ShortcutDef] = &[
    // Playback
    ShortcutDef {
        action: "play-pause",
        display_name: gettext_noop("Play / Pause"),
        category: gettext_noop("Playback"),
        default_accel: "space",
        single_key: true,
    },
    ShortcutDef {
        action: "seek-forward",
        display_name: gettext_noop("Seek forward 5 s"),
        category: gettext_noop("Playback"),
        default_accel: "Right",
        single_key: true,
    },
    ShortcutDef {
        action: "seek-backward",
        display_name: gettext_noop("Seek backward 5 s"),
        category: gettext_noop("Playback"),
        default_accel: "Left",
        single_key: true,
    },
    ShortcutDef {
        action: "seek-forward-long",
        display_name: gettext_noop("Seek forward 30 s"),
        category: gettext_noop("Playback"),
        default_accel: "<Shift>Right",
        single_key: true,
    },
    ShortcutDef {
        action: "seek-backward-long",
        display_name: gettext_noop("Seek backward 30 s"),
        category: gettext_noop("Playback"),
        default_accel: "<Shift>Left",
        single_key: true,
    },
    ShortcutDef {
        action: "speed-up",
        display_name: gettext_noop("Increase speed"),
        category: gettext_noop("Playback"),
        default_accel: "bracketright",
        single_key: true,
    },
    ShortcutDef {
        action: "speed-down",
        display_name: gettext_noop("Decrease speed"),
        category: gettext_noop("Playback"),
        default_accel: "bracketleft",
        single_key: true,
    },
    ShortcutDef {
        action: "speed-reset",
        display_name: gettext_noop("Reset speed to 1×"),
        category: gettext_noop("Playback"),
        default_accel: "BackSpace",
        single_key: true,
    },
    ShortcutDef {
        action: "chapter-next",
        display_name: gettext_noop("Next chapter"),
        category: gettext_noop("Playback"),
        default_accel: "Page_Up",
        single_key: true,
    },
    ShortcutDef {
        action: "chapter-prev",
        display_name: gettext_noop("Previous chapter"),
        category: gettext_noop("Playback"),
        default_accel: "Page_Down",
        single_key: true,
    },
    ShortcutDef {
        action: "playlist-next",
        display_name: gettext_noop("Next in playlist"),
        category: gettext_noop("Playback"),
        default_accel: "greater",
        single_key: true,
    },
    ShortcutDef {
        action: "playlist-prev",
        display_name: gettext_noop("Previous in playlist"),
        category: gettext_noop("Playback"),
        default_accel: "less",
        single_key: true,
    },
    ShortcutDef {
        action: "frame-step",
        display_name: gettext_noop("Next frame"),
        category: gettext_noop("Playback"),
        default_accel: "period",
        single_key: true,
    },
    ShortcutDef {
        action: "frame-back-step",
        display_name: gettext_noop("Previous frame"),
        category: gettext_noop("Playback"),
        default_accel: "comma",
        single_key: true,
    },
    ShortcutDef {
        action: "revert-seek",
        display_name: gettext_noop("Undo last seek"),
        category: gettext_noop("Playback"),
        default_accel: "<Shift>BackSpace",
        single_key: true,
    },
    // Video
    ShortcutDef {
        action: "fullscreen",
        display_name: gettext_noop("Toggle fullscreen"),
        category: gettext_noop("Video"),
        default_accel: "f",
        single_key: true,
    },
    ShortcutDef {
        action: "fullscreen-f11",
        display_name: gettext_noop("Toggle fullscreen (F11)"),
        category: gettext_noop("Video"),
        default_accel: "F11",
        single_key: true,
    },
    ShortcutDef {
        action: "exit-fullscreen",
        display_name: gettext_noop("Exit fullscreen"),
        category: gettext_noop("Video"),
        default_accel: "Escape",
        single_key: true,
    },
    ShortcutDef {
        action: "deinterlace",
        display_name: gettext_noop("Toggle deinterlace"),
        category: gettext_noop("Video"),
        default_accel: "d",
        single_key: true,
    },
    ShortcutDef {
        action: "hwdec",
        display_name: gettext_noop("Toggle hardware decoding"),
        category: gettext_noop("Video"),
        default_accel: "<Ctrl>h",
        single_key: true,
    },
    ShortcutDef {
        action: "panscan-up",
        display_name: gettext_noop("Increase panscan"),
        category: gettext_noop("Video"),
        default_accel: "e",
        single_key: true,
    },
    ShortcutDef {
        action: "panscan-down",
        display_name: gettext_noop("Decrease panscan"),
        category: gettext_noop("Video"),
        default_accel: "w",
        single_key: true,
    },
    ShortcutDef {
        action: "crop",
        display_name: gettext_noop("Crop video"),
        category: gettext_noop("Video"),
        default_accel: "<Ctrl>c",
        single_key: true,
    },
    // Audio
    ShortcutDef {
        action: "volume-up",
        display_name: gettext_noop("Volume up"),
        category: gettext_noop("Audio"),
        default_accel: "Up",
        single_key: true,
    },
    ShortcutDef {
        action: "volume-down",
        display_name: gettext_noop("Volume down"),
        category: gettext_noop("Audio"),
        default_accel: "Down",
        single_key: true,
    },
    ShortcutDef {
        action: "mute",
        display_name: gettext_noop("Toggle mute"),
        category: gettext_noop("Audio"),
        default_accel: "m",
        single_key: true,
    },
    ShortcutDef {
        action: "audio-passthrough",
        display_name: gettext_noop("Toggle audio passthrough"),
        category: gettext_noop("Audio"),
        default_accel: "<Ctrl>p",
        single_key: true,
    },
    // Subtitles
    ShortcutDef {
        action: "secondary-sub",
        display_name: gettext_noop("Toggle secondary subtitles"),
        category: gettext_noop("Subtitles"),
        default_accel: "v",
        single_key: true,
    },
    ShortcutDef {
        action: "sub-seek-forward",
        display_name: gettext_noop("Next subtitle"),
        category: gettext_noop("Subtitles"),
        default_accel: "<Ctrl>Right",
        single_key: true,
    },
    ShortcutDef {
        action: "sub-seek-backward",
        display_name: gettext_noop("Previous subtitle"),
        category: gettext_noop("Subtitles"),
        default_accel: "<Ctrl>Left",
        single_key: true,
    },
    ShortcutDef {
        action: "cycle-cc",
        display_name: gettext_noop("Cycle Closed Captions"),
        category: gettext_noop("Subtitles"),
        default_accel: "c",
        single_key: true,
    },
    // Screenshots
    ShortcutDef {
        action: "screenshot",
        display_name: gettext_noop("Screenshot"),
        category: gettext_noop("Screenshots"),
        default_accel: "s",
        single_key: true,
    },
    // Music
    ShortcutDef {
        action: "music-shuffle",
        display_name: gettext_noop("Toggle shuffle"),
        category: gettext_noop("Music"),
        default_accel: "z",
        single_key: true,
    },
    ShortcutDef {
        action: "music-repeat",
        display_name: gettext_noop("Cycle repeat mode"),
        category: gettext_noop("Music"),
        default_accel: "r",
        single_key: true,
    },
    ShortcutDef {
        action: "music-replaygain",
        display_name: gettext_noop("Cycle ReplayGain"),
        category: gettext_noop("Music"),
        default_accel: "<Ctrl>r",
        single_key: true,
    },
    ShortcutDef {
        action: "music-gapless",
        display_name: gettext_noop("Toggle gapless"),
        category: gettext_noop("Music"),
        default_accel: "g",
        single_key: true,
    },
    // Navigation
    ShortcutDef {
        action: "chapter-next",
        display_name: gettext_noop("Next chapter"),
        category: gettext_noop("Navigation"),
        default_accel: "exclam",
        single_key: true,
    },
    ShortcutDef {
        action: "chapter-prev",
        display_name: gettext_noop("Previous chapter"),
        category: gettext_noop("Navigation"),
        default_accel: "at",
        single_key: true,
    },
    // Other
    ShortcutDef {
        action: "info-overlay",
        display_name: gettext_noop("Toggle file info overlay"),
        category: gettext_noop("Other"),
        default_accel: "i",
        single_key: true,
    },
    ShortcutDef {
        action: "open-file",
        display_name: gettext_noop("Open file"),
        category: gettext_noop("Other"),
        default_accel: "<Ctrl>o",
        single_key: true,
    },
    ShortcutDef {
        action: "open-url",
        display_name: gettext_noop("Open URL"),
        category: gettext_noop("Other"),
        default_accel: "<Ctrl>u",
        single_key: true,
    },
    ShortcutDef {
        action: "paste-play",
        display_name: gettext_noop("Paste and play"),
        category: gettext_noop("Other"),
        default_accel: "<Ctrl>v",
        single_key: true,
    },
    ShortcutDef {
        action: "goto-time",
        display_name: gettext_noop("Go to time"),
        category: gettext_noop("Other"),
        default_accel: "<Ctrl>t",
        single_key: true,
    },
    ShortcutDef {
        action: "cycle-edition",
        display_name: gettext_noop("Cycle edition"),
        category: gettext_noop("Other"),
        // Was `e` → clashed with panscan-up. mpv default is Shift+E.
        default_accel: "<Shift>e",
        single_key: true,
    },
    ShortcutDef {
        action: "toggle-files-sidebar",
        display_name: gettext_noop("Toggle files sidebar"),
        category: gettext_noop("Other"),
        default_accel: "F9",
        single_key: true,
    },
    ShortcutDef {
        action: "toggle-playlist-sidebar",
        display_name: gettext_noop("Toggle playlist sidebar"),
        category: gettext_noop("Other"),
        default_accel: "<Shift>F9",
        single_key: true,
    },
    ShortcutDef {
        action: "media-info",
        display_name: gettext_noop("Media information"),
        category: gettext_noop("Other"),
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

/// Accel→action map for the bare single-key bindings only. Modified bindings are
/// GTK accelerators; bare keys are dispatched by the window key controller and
/// must be filtered so text entries keep normal typing.
#[must_use]
pub fn build_single_key_map(custom: &str) -> std::collections::HashMap<String, String> {
    build_accel_map(SHORTCUTS, custom, true)
}
