//! Typed setting-key wrappers — `Settings::get_bool(VOLUME)` fails to compile
//! because `VOLUME` is a `DoubleKey`, catching access mismatches at build time.

#[derive(Clone, Copy)]
pub struct BoolKey(pub &'static str);
#[derive(Clone, Copy)]
pub struct IntKey(pub &'static str);
#[derive(Clone, Copy)]
pub struct DoubleKey(pub &'static str);
#[derive(Clone, Copy)]
pub struct StringKey(pub &'static str);

// UI mode
pub const ADVANCED_MODE: BoolKey = BoolKey("advanced-mode");

// Dialog sizes
pub const PREFS_WINDOW_WIDTH: IntKey = IntKey("prefs-window-width");
pub const PREFS_WINDOW_HEIGHT: IntKey = IntKey("prefs-window-height");
pub const APPEARANCE_WINDOW_WIDTH: IntKey = IntKey("appearance-window-width");
pub const APPEARANCE_WINDOW_HEIGHT: IntKey = IntKey("appearance-window-height");
pub const AUDIO_PREFS_WINDOW_WIDTH: IntKey = IntKey("audio-prefs-window-width");
pub const AUDIO_PREFS_WINDOW_HEIGHT: IntKey = IntKey("audio-prefs-window-height");
pub const AUDIO_APPEARANCE_WINDOW_WIDTH: IntKey = IntKey("audio-appearance-window-width");
pub const AUDIO_APPEARANCE_WINDOW_HEIGHT: IntKey = IntKey("audio-appearance-window-height");

pub const VOLUME: DoubleKey = DoubleKey("volume");
pub const MUTE: BoolKey = BoolKey("mute");

// Video
pub const HWDEC: StringKey = StringKey("hwdec");
pub const AUTO_WINDOW_RESIZE: BoolKey = BoolKey("auto-window-resize");

// Audio output & language
pub const AUDIO_OUTPUT: StringKey = StringKey("audio-output");
pub const AUDIO_NORMALIZE: BoolKey = BoolKey("audio-normalize");
pub const AUDIO_PITCH_CORRECTION: BoolKey = BoolKey("audio-pitch-correction");
pub const VOLUME_MAX: IntKey = IntKey("volume-max");
pub const PREFERRED_AUDIO_LANG: StringKey = StringKey("preferred-audio-lang");
pub const AUDIO_CHANNELS: StringKey = StringKey("audio-channels");

// Subtitles
pub const SUB_FONT: StringKey = StringKey("sub-font");

// Playback
pub const RESUME_PLAYBACK: BoolKey = BoolKey("resume-playback");
pub const LOOP_MODE: StringKey = StringKey("loop-mode");
pub const KEEP_OPEN: StringKey = StringKey("keep-open");
pub const KEEP_OPEN_PAUSE: BoolKey = BoolKey("keep-open-pause");
pub const IDLE_MODE: StringKey = StringKey("idle-mode");

// Audio effects
pub const NOISE_STRENGTH: DoubleKey = DoubleKey("noise-strength");
pub const NOISE_MODEL: IntKey = IntKey("noise-model");
pub const NOISE_VOICE_RECOVERY: DoubleKey = DoubleKey("noise-voice-recovery");
pub const NOISE_MODEL_BLEND: BoolKey = BoolKey("noise-model-blend");
pub const EQ_BANDS: StringKey = StringKey("eq-bands");
pub const EQ_PRESET: StringKey = StringKey("eq-preset");

// Render profile (simple mode preset)
pub const RENDER_PROFILE: StringKey = StringKey("render-profile");

// Video sync & interpolation
pub const VIDEO_SYNC: StringKey = StringKey("video-sync");
pub const INTERPOLATION: BoolKey = BoolKey("interpolation");
pub const TSCALE: StringKey = StringKey("tscale");

pub const HDR_COMPUTE_PEAK: BoolKey = BoolKey("hdr-compute-peak");

pub const BUILTIN_UPSCALER: StringKey = StringKey("builtin-upscaler");
pub const SHADER_FSR_SHARPNESS: DoubleKey = DoubleKey("shader-fsr-sharpness");
pub const SHADER_CAS_SHARPENING: DoubleKey = DoubleKey("shader-cas-sharpening");
pub const ANIME4K_PRESET: StringKey = StringKey("anime4k-preset");
pub const NEURAL_UPSCALER: StringKey = StringKey("neural-upscaler");

// GPU / advanced
pub const CUSTOM_MPV_OPTIONS: StringKey = StringKey("custom-mpv-options");
pub const GPU_DUMB_MODE: BoolKey = BoolKey("gpu-dumb-mode");
/// Video output mode: `"auto"` (try GL/offload, fall back to software when GL is
/// not viable — the default), `"gl"` (force OpenGL/offload), `"sw"` (force the
/// software/no-GL render path). Read at render setup; default `"auto"`.
pub const RENDER_MODE: StringKey = StringKey("render-mode");

pub const ICC_INTENT: IntKey = IntKey("icc-intent");

// Seekbar
pub const SEEKBAR_THUMBNAIL: BoolKey = BoolKey("seekbar-thumbnail");

// Custom keybindings
pub const CUSTOM_KEYBINDINGS: StringKey = StringKey("custom-keybindings");

// Playlist
pub const PLAYLIST_VIEW: StringKey = StringKey("playlist-view");
pub const PLAYLIST_FOLDER_RECURSIVE: BoolKey = BoolKey("playlist-folder-recursive");
pub const PLAYLIST_SORT: StringKey = StringKey("playlist-sort");
pub const PLAYLIST_SORT_DESC: BoolKey = BoolKey("playlist-sort-desc");
pub const PLAYLIST_LAST_FOLDER: StringKey = StringKey("playlist-last-folder");
pub const PLAYLIST_SHUFFLE: BoolKey = BoolKey("playlist-shuffle");

// UI appearance (video)
pub const UI_COLOR_SOURCE: StringKey = StringKey("ui-color-source");
pub const UI_BG_COLOR: StringKey = StringKey("ui-bg-color");
pub const UI_BG_OPACITY: DoubleKey = DoubleKey("ui-bg-opacity");
pub const UI_MENU_OPACITY: DoubleKey = DoubleKey("ui-menu-opacity");
pub const UI_OSD_SHADOW: BoolKey = BoolKey("ui-osd-shadow");
pub const UI_FONT_COLOR: StringKey = StringKey("ui-font-color");
pub const UI_ICON_COLOR: StringKey = StringKey("ui-icon-color");
pub const UI_DIALOG_COLOR: StringKey = StringKey("ui-dialog-color");
pub const UI_CONTROLS_RADIUS: IntKey = IntKey("ui-controls-radius");
pub const UI_CONTROLS_MARGIN_BOTTOM: IntKey = IntKey("ui-controls-margin-bottom");
pub const UI_CONTROLS_MARGIN_SIDE: IntKey = IntKey("ui-controls-margin-side");
pub const UI_CONTROLS_PADDING_TOP: IntKey = IntKey("ui-controls-padding-top");
pub const UI_CONTROLS_PADDING_BOTTOM: IntKey = IntKey("ui-controls-padding-bottom");
pub const UI_CONTROLS_PADDING_LEFT: IntKey = IntKey("ui-controls-padding-left");
pub const UI_CONTROLS_PADDING_RIGHT: IntKey = IntKey("ui-controls-padding-right");
pub const UI_BUTTON_CUSTOM: BoolKey = BoolKey("ui-button-custom");
pub const UI_BUTTON_SIZE: IntKey = IntKey("ui-button-size");
pub const UI_BUTTON_RADIUS: IntKey = IntKey("ui-button-radius");
pub const UI_WINDOW_RADIUS_CUSTOM: BoolKey = BoolKey("ui-window-radius-custom");
pub const UI_WINDOW_RADIUS: IntKey = IntKey("ui-window-radius");
pub const UI_WINDOW_BUTTON_CUSTOM_SIZE: BoolKey = BoolKey("ui-window-btn-custom-size");
pub const UI_SHOW_VOLUME: BoolKey = BoolKey("ui-show-volume");
pub const UI_SHOW_SPEED: BoolKey = BoolKey("ui-show-speed");
pub const UI_SHOW_VOICE: BoolKey = BoolKey("ui-show-voice");
pub const UI_SHOW_PLAY: BoolKey = BoolKey("ui-show-play");
pub const UI_SHOW_PREV: BoolKey = BoolKey("ui-show-prev");
pub const UI_SHOW_NEXT: BoolKey = BoolKey("ui-show-next");
pub const UI_SHOW_SUBTITLE: BoolKey = BoolKey("ui-show-subtitle");
pub const UI_SHOW_AUDIO_TRACK: BoolKey = BoolKey("ui-show-audio-track");
pub const UI_SHOW_FULLSCREEN: BoolKey = BoolKey("ui-show-fullscreen");
pub const UI_SHOW_PIN: BoolKey = BoolKey("ui-show-pin");
pub const UI_SHOW_OPEN: BoolKey = BoolKey("ui-show-open");
pub const UI_SHOW_PLAYLIST: BoolKey = BoolKey("ui-show-playlist");
pub const UI_SHOW_MENU: BoolKey = BoolKey("ui-show-menu");
pub const UI_ZONE_TOP_LEFT: StringKey = StringKey("ui-zone-top-left");
pub const UI_ZONE_TOP_RIGHT: StringKey = StringKey("ui-zone-top-right");
pub const UI_ZONE_BOTTOM_LEFT: StringKey = StringKey("ui-zone-bottom-left");
pub const UI_ZONE_BOTTOM_CENTER: StringKey = StringKey("ui-zone-bottom-center");
pub const UI_ZONE_BOTTOM_RIGHT: StringKey = StringKey("ui-zone-bottom-right");

// Audio-specific appearance
pub const AUDIO_UI_COLOR_SOURCE: StringKey = StringKey("audio-ui-color-source");
pub const AUDIO_UI_BG_COLOR: StringKey = StringKey("audio-ui-bg-color");
pub const AUDIO_UI_FONT_COLOR: StringKey = StringKey("audio-ui-font-color");
pub const AUDIO_UI_ICON_COLOR: StringKey = StringKey("audio-ui-icon-color");
pub const AUDIO_UI_DIALOG_COLOR: StringKey = StringKey("audio-ui-dialog-color");

// Music mode
pub const REPLAYGAIN: StringKey = StringKey("replaygain");
pub const REPLAYGAIN_PREAMP: DoubleKey = DoubleKey("replaygain-preamp");
pub const GAPLESS_AUDIO: BoolKey = BoolKey("gapless-audio");
pub const AUDIO_VOLUME: IntKey = IntKey("audio-volume");

// High-contrast text mode
pub const AUDIO_HIGH_CONTRAST: BoolKey = BoolKey("audio-high-contrast");

// Sleep timer action
pub const SLEEP_TIMER_ACTION: StringKey = StringKey("sleep-timer-action");
pub const SLEEP_TIMER_MINUTES: IntKey = IntKey("sleep-timer-minutes");

// "Play on a TV": the frame height a decoded file is sent at, 0 = its own
pub const TV_RESOLUTION: IntKey = IntKey("tv-resolution");

// Library
pub const LIBRARY_GROUPING: StringKey = StringKey("library-grouping");
pub const LIBRARY_COLUMN_WIDTHS: StringKey = StringKey("library-column-widths");
pub const PLAYLIST_COLUMN_WIDTHS: StringKey = StringKey("playlist-column-widths");

// Desktop integration
pub const TASKBAR_PROGRESS: BoolKey = BoolKey("taskbar-progress");

/// Use the current cover as a blurred background behind the audio controls.
pub const AUDIO_ALBUM_ART_BG: BoolKey = BoolKey("audio-album-art-bg");

// File browser
pub const FILES_LAST_DIR: StringKey = StringKey("files-last-dir");
