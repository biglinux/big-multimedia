//! The settings key catalog: each key's name, type and default, written once.
//! `Settings::get_bool(VOLUME)` fails to compile because `VOLUME` is a
//! `DoubleKey`, catching access mismatches at build time.

use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct BoolKey(pub &'static str);
#[derive(Clone, Copy)]
pub struct IntKey(pub &'static str);
#[derive(Clone, Copy)]
pub struct DoubleKey(pub &'static str);
#[derive(Clone, Copy)]
pub struct StringKey(pub &'static str);

/// What a stored value must parse as; text takes any value the file format allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ValueType {
    Bool,
    Int,
    Double,
    Text,
}

impl BoolKey {
    const VALUE_TYPE: ValueType = ValueType::Bool;
}
impl IntKey {
    const VALUE_TYPE: ValueType = ValueType::Int;
}
impl DoubleKey {
    const VALUE_TYPE: ValueType = ValueType::Double;
}
impl StringKey {
    const VALUE_TYPE: ValueType = ValueType::Text;
}

/// One key of [`CATALOG`]. A non-empty `choices` is the only values the key takes.
#[derive(Debug)]
pub(super) struct Entry {
    pub(super) name: &'static str,
    pub(super) default: &'static str,
    pub(super) value_type: ValueType,
    pub(super) choices: &'static [&'static str],
}

macro_rules! catalog {
    ($(
        $(#[$doc:meta])*
        $constant:ident: $key:ident($name:literal) = $default:literal
            $(, one of [$($choice:literal),+])?;
    )*) => {
        $(
            $(#[$doc])*
            pub const $constant: $key = $key($name);
        )*

        /// Every key with the text a new file holds for it.
        pub(super) const CATALOG: &[Entry] = &[$(
            Entry {
                name: $name,
                default: $default,
                value_type: $key::VALUE_TYPE,
                choices: &[$($($choice),+)?],
            },
        )*];
    };
}

catalog! {
    // Dialog sizes
    APPEARANCE_WINDOW_WIDTH: IntKey("appearance-window-width") = "600";
    APPEARANCE_WINDOW_HEIGHT: IntKey("appearance-window-height") = "700";
    AUDIO_APPEARANCE_WINDOW_WIDTH: IntKey("audio-appearance-window-width") = "500";
    AUDIO_APPEARANCE_WINDOW_HEIGHT: IntKey("audio-appearance-window-height") = "400";

    VOLUME: DoubleKey("volume") = "1";

    // Video
    HWDEC: StringKey("hwdec") = "auto-safe";
    AUTO_WINDOW_RESIZE: BoolKey("auto-window-resize") = "true";
    DEINTERLACE: BoolKey("deinterlace") = "false";
    STOP_SCREENSAVER: StringKey("stop-screensaver") = "yes";

    // Audio output & language
    AUDIO_OUTPUT: StringKey("audio-output") = "";
    AUDIO_NORMALIZE: BoolKey("audio-normalize") = "false";
    AUDIO_PITCH_CORRECTION: BoolKey("audio-pitch-correction") = "true";
    VOLUME_MAX: IntKey("volume-max") = "150";
    PREFERRED_AUDIO_LANG: StringKey("preferred-audio-lang") = "";
    AUDIO_CHANNELS: StringKey("audio-channels") = "";

    // Subtitles
    SUB_FONT: StringKey("sub-font") = "";
    PREFERRED_SUB_LANG: StringKey("preferred-sub-lang") = "";
    SUB_AUTO: StringKey("sub-auto") = "fuzzy";
    SUBTITLE_FONT_SIZE: IntKey("subtitle-font-size") = "36";
    SUB_BOLD: BoolKey("sub-bold") = "false";
    SUB_COLOR: StringKey("sub-color") = "#FFFFFFFF";
    SUB_BORDER_STYLE: StringKey("sub-border-style") = "outline-and-shadow";
    SUB_BORDER_SIZE: DoubleKey("sub-border-size") = "2.5";
    SUB_BORDER_COLOR: StringKey("sub-border-color") = "#FF000000";
    SUB_SHADOW_OFFSET: DoubleKey("sub-shadow-offset") = "0.0";
    SUB_SHADOW_COLOR: StringKey("sub-shadow-color") = "#80000000";
    SUB_MARGIN_Y: IntKey("sub-margin-y") = "22";
    SUB_SCALE_WITH_WINDOW: BoolKey("sub-scale-with-window") = "true";
    SUB_ASS_SCALE_WITH_WINDOW: BoolKey("sub-ass-scale-with-window") = "false";
    SUB_FILTER_SDH: BoolKey("sub-filter-sdh") = "false";
    SUB_FILTER_SDH_HARDER: BoolKey("sub-filter-sdh-harder") = "false";
    SUB_GAUSS: DoubleKey("sub-gauss") = "0.0";
    SUB_GRAY: BoolKey("sub-gray") = "false";

    // Playback
    RESUME_PLAYBACK: BoolKey("resume-playback") = "true";
    LOOP_MODE: StringKey("loop-mode") = "no";
    IDLE_MODE: StringKey("idle-mode") = "yes";
    KEEP_OPEN: StringKey("keep-open") = "yes";
    KEEP_OPEN_PAUSE: BoolKey("keep-open-pause") = "false";

    // Audio effects
    NOISE_STRENGTH: DoubleKey("noise-strength") = "0.0";
    NOISE_MODEL: IntKey("noise-model") = "0";
    NOISE_MONO: BoolKey("noise-mono") = "false";
    /// Keep only what the denoiser's voice detector hears, at
    /// [`NOISE_VOICE_SENSITIVITY`] 1 (sensitive) to 5 (strict).
    NOISE_VOICE_ONLY: BoolKey("noise-voice-only") = "false";
    NOISE_VOICE_SENSITIVITY: IntKey("noise-voice-sensitivity") = "3",
        one of ["1", "2", "3", "4", "5"];
    EQ_BANDS: StringKey("eq-bands") = "";
    EQ_PRESET: StringKey("eq-preset") = "flat";

    // Render profile (simple mode preset)
    RENDER_PROFILE: StringKey("render-profile") = "default";

    HDR_COMPUTE_PEAK: BoolKey("hdr-compute-peak") = "false";
    TONE_MAPPING: StringKey("tone-mapping") = "auto";
    TARGET_PEAK: IntKey("target-peak") = "0";
    GAMUT_MAPPING_MODE: StringKey("gamut-mapping-mode") = "auto";

    BUILTIN_UPSCALER: StringKey("builtin-upscaler") = "";
    // FSR reads 0 as its 0.2 sharpness.
    SHADER_FSR_SHARPNESS: DoubleKey("shader-fsr-sharpness") = "0";
    SHADER_CAS_SHARPENING: DoubleKey("shader-cas-sharpening") = "0";
    ANIME4K_PRESET: StringKey("anime4k-preset") = "";
    NEURAL_UPSCALER: StringKey("neural-upscaler") = "";

    // GPU / advanced
    CUSTOM_MPV_OPTIONS: StringKey("custom-mpv-options") = "";
    GPU_DUMB_MODE: BoolKey("gpu-dumb-mode") = "false";
    GLSL_SHADERS: StringKey("glsl-shaders") = "";
    CACHE: BoolKey("cache") = "false";
    CACHE_SECS: IntKey("cache-secs") = "30";
    CACHE_PAUSE: BoolKey("cache-pause") = "true";
    CACHE_ON_DISK: BoolKey("cache-on-disk") = "false";
    DEMUXER_READAHEAD_SECS: DoubleKey("demuxer-readahead-secs") = "1.0";
    SCREENSHOT_PNG_COMPRESSION: IntKey("screenshot-png-compression") = "7";
    /// Folder screenshots are saved in; empty for the Pictures folder.
    SCREENSHOT_DIRECTORY: StringKey("screenshot-directory") = "";
    SCREENSHOT_TAG_COLORSPACE: BoolKey("screenshot-tag-colorspace") = "true";
    SCREENSHOT_HIGH_BIT_DEPTH: BoolKey("screenshot-high-bit-depth") = "true";
    /// Video output mode: `"auto"` (try GL/offload, fall back to software when GL is
    /// not viable), `"gl"` (force OpenGL/offload), `"sw"` (force the
    /// software/no-GL render path). Read at render setup.
    RENDER_MODE: StringKey("render-mode") = "auto", one of ["auto", "gl", "sw"];

    ICC_PROFILE: StringKey("icc-profile") = "";
    ICC_INTENT: IntKey("icc-intent") = "0";

    // Seekbar
    SEEKBAR_THUMBNAIL: BoolKey("seekbar-thumbnail") = "true";

    // Custom keybindings
    CUSTOM_KEYBINDINGS: StringKey("custom-keybindings") = "";

    // Playlist
    PLAYLIST_VIEW: StringKey("playlist-view") = "list";
    PLAYLIST_FOLDER_RECURSIVE: BoolKey("playlist-folder-recursive") = "false";
    PLAYLIST_SORT: StringKey("playlist-sort") = "name";
    PLAYLIST_SORT_DESC: BoolKey("playlist-sort-desc") = "false";
    PLAYLIST_LAST_FOLDER: StringKey("playlist-last-folder") = "";
    PLAYLIST_SHUFFLE: BoolKey("playlist-shuffle") = "false";
    /// The playlist's one-time "drag to reorder" line was closed.
    PLAYLIST_REORDER_HINT_SEEN: BoolKey("playlist-reorder-hint-seen") = "false";
    /// The folders the Files tab shows and searches, a JSON list; empty for
    /// Videos, Downloads and the home folder.
    VIDEO_LIBRARY_FOLDERS: StringKey("video-library-folders") = "";

    // UI appearance (video); the chrome takes its colors from the video.
    UI_COLOR_SOURCE: StringKey("ui-color-source") = "gradient";
    UI_BG_COLOR: StringKey("ui-bg-color") = "#2A2A30";
    UI_BG_OPACITY: DoubleKey("ui-bg-opacity") = "0.90";
    UI_MENU_OPACITY: DoubleKey("ui-menu-opacity") = "0.95";
    UI_OSD_SHADOW: BoolKey("ui-osd-shadow") = "false";
    UI_FONT_COLOR: StringKey("ui-font-color") = "#FFFFFF";
    UI_ICON_COLOR: StringKey("ui-icon-color") = "#FFFFFF";
    UI_DIALOG_COLOR: StringKey("ui-dialog-color") = "#2A2A30";
    UI_CONTROLS_RADIUS: IntKey("ui-controls-radius") = "0";
    UI_CONTROLS_MARGIN_BOTTOM: IntKey("ui-controls-margin-bottom") = "0";
    UI_CONTROLS_MARGIN_SIDE: IntKey("ui-controls-margin-side") = "0";
    UI_CONTROLS_PADDING_TOP: IntKey("ui-controls-padding-top") = "0";
    UI_CONTROLS_PADDING_BOTTOM: IntKey("ui-controls-padding-bottom") = "0";
    UI_CONTROLS_PADDING_LEFT: IntKey("ui-controls-padding-left") = "0";
    UI_CONTROLS_PADDING_RIGHT: IntKey("ui-controls-padding-right") = "0";
    UI_BUTTON_CUSTOM: BoolKey("ui-button-custom") = "false";
    UI_BUTTON_SIZE: IntKey("ui-button-size") = "36";
    UI_BUTTON_RADIUS: IntKey("ui-button-radius") = "18";
    UI_WINDOW_RADIUS_CUSTOM: BoolKey("ui-window-radius-custom") = "false";
    UI_WINDOW_RADIUS: IntKey("ui-window-radius") = "12";
    UI_WINDOW_BUTTON_CUSTOM_SIZE: BoolKey("ui-window-btn-custom-size") = "false";
    UI_SHOW_VOLUME: BoolKey("ui-show-volume") = "true";
    UI_SHOW_SPEED: BoolKey("ui-show-speed") = "true";
    UI_SHOW_VOICE: BoolKey("ui-show-voice") = "true";
    UI_SHOW_PLAY: BoolKey("ui-show-play") = "true";
    UI_SHOW_PREV: BoolKey("ui-show-prev") = "true";
    UI_SHOW_NEXT: BoolKey("ui-show-next") = "true";
    UI_SHOW_SUBTITLE: BoolKey("ui-show-subtitle") = "true";
    UI_SHOW_AUDIO_TRACK: BoolKey("ui-show-audio-track") = "true";
    UI_SHOW_TV: BoolKey("ui-show-tv") = "true";
    UI_SHOW_FULLSCREEN: BoolKey("ui-show-fullscreen") = "true";
    UI_SHOW_PIN: BoolKey("ui-show-pin") = "true";
    UI_SHOW_OPEN: BoolKey("ui-show-open") = "true";
    UI_SHOW_PLAYLIST: BoolKey("ui-show-playlist") = "true";
    UI_SHOW_MENU: BoolKey("ui-show-menu") = "true";
    UI_ZONE_TOP_LEFT: StringKey("ui-zone-top-left") = "pin,open";
    UI_ZONE_TOP_RIGHT: StringKey("ui-zone-top-right") = "menu,playlist";
    UI_ZONE_BOTTOM_LEFT: StringKey("ui-zone-bottom-left") = "volume,speed,voice";
    UI_ZONE_BOTTOM_CENTER: StringKey("ui-zone-bottom-center") = "prev,play,next";
    UI_ZONE_BOTTOM_RIGHT: StringKey("ui-zone-bottom-right") =
        "subtitle,audio-track,tv,fullscreen";

    // Audio-specific appearance
    AUDIO_UI_COLOR_SOURCE: StringKey("audio-ui-color-source") = "gtk-default";
    AUDIO_UI_BG_COLOR: StringKey("audio-ui-bg-color") = "#2A2A30";
    AUDIO_UI_FONT_COLOR: StringKey("audio-ui-font-color") = "#FFFFFF";
    AUDIO_UI_ICON_COLOR: StringKey("audio-ui-icon-color") = "#FFFFFF";
    AUDIO_UI_DIALOG_COLOR: StringKey("audio-ui-dialog-color") = "#2A2A30";

    // Music mode
    REPLAYGAIN: StringKey("replaygain") = "track";
    REPLAYGAIN_PREAMP: DoubleKey("replaygain-preamp") = "0.0";
    GAPLESS_AUDIO: BoolKey("gapless-audio") = "false";
    AUDIO_VOLUME: IntKey("audio-volume") = "100";

    // High-contrast text mode
    AUDIO_HIGH_CONTRAST: BoolKey("audio-high-contrast") = "false";

    // Sleep timer action
    SLEEP_TIMER_ACTION: StringKey("sleep-timer-action") = "pause";
    SLEEP_TIMER_MINUTES: IntKey("sleep-timer-minutes") = "30";

    // "Play on a TV": the frame height a decoded file is sent at, 0 = its own
    TV_RESOLUTION: IntKey("tv-resolution") = "0", one of ["0", "720", "1080", "1440", "2160"];

    // Library
    LIBRARY_GROUPING: StringKey("library-grouping") = "";
    PLAYLIST_COLUMN_WIDTHS: StringKey("playlist-column-widths") = "";

    // Desktop integration
    TASKBAR_PROGRESS: BoolKey("taskbar-progress") = "true";
    /// Show an icon with playback controls in the system tray; off unless chosen.
    TRAY_ICON: BoolKey("tray-icon") = "false";

    /// Use the current cover as a blurred background behind the audio controls.
    AUDIO_ALBUM_ART_BG: BoolKey("audio-album-art-bg") = "true";

    // File browser
    FILES_LAST_DIR: StringKey("files-last-dir") = "";

    /// Radio Browser hosts, comma-separated; empty finds them through DNS.
    RADIO_API_SERVERS: StringKey("radio-api-servers") =
        "de2.api.radio-browser.info,nl1.api.radio-browser.info,at1.api.radio-browser.info,de1.api.radio-browser.info";
}

/// Every key with its default, as a new file holds them.
pub(super) fn defaults() -> HashMap<String, String> {
    CATALOG
        .iter()
        .map(|entry| (entry.name.to_owned(), entry.default.to_owned()))
        .collect()
}

/// The catalog entry of `name`, or `None` for a key the players do not know.
pub(super) fn entry(name: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|entry| entry.name == name)
}
