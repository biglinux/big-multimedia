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
pub const PLAYLIST_DIALOG_WIDTH: IntKey = IntKey("playlist-dialog-width");
pub const PLAYLIST_DIALOG_HEIGHT: IntKey = IntKey("playlist-dialog-height");
pub const TAG_EDITOR_WIDTH: IntKey = IntKey("tag-editor-width");
pub const TAG_EDITOR_HEIGHT: IntKey = IntKey("tag-editor-height");

// Window state
pub const WINDOW_WIDTH: IntKey = IntKey("window-width");
pub const WINDOW_HEIGHT: IntKey = IntKey("window-height");
pub const WINDOW_MAXIMIZED: BoolKey = BoolKey("window-maximized");
pub const VOLUME: DoubleKey = DoubleKey("volume");
pub const MUTE: BoolKey = BoolKey("mute");
pub const OPEN_NEW_WINDOWS: BoolKey = BoolKey("open-new-windows");

// Video
pub const HWDEC: StringKey = StringKey("hwdec");
pub const DEINTERLACE: BoolKey = BoolKey("deinterlace");
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
pub const SUBTITLE_FONT_SIZE: IntKey = IntKey("subtitle-font-size");
pub const SUB_BOLD: BoolKey = BoolKey("sub-bold");
pub const SUB_COLOR: StringKey = StringKey("sub-color");
pub const SUB_BORDER_SIZE: DoubleKey = DoubleKey("sub-border-size");
pub const SUB_BORDER_COLOR: StringKey = StringKey("sub-border-color");
pub const SUB_SHADOW_COLOR: StringKey = StringKey("sub-shadow-color");
pub const SUB_SHADOW_OFFSET: DoubleKey = DoubleKey("sub-shadow-offset");
pub const SUB_MARGIN_Y: IntKey = IntKey("sub-margin-y");
pub const PREFERRED_SUB_LANG: StringKey = StringKey("preferred-sub-lang");
pub const SUB_AUTO: StringKey = StringKey("sub-auto");
pub const SUB_BORDER_STYLE: StringKey = StringKey("sub-border-style");
pub const SUB_SCALE_WITH_WINDOW: BoolKey = BoolKey("sub-scale-with-window");
pub const SUB_ASS_SCALE_WITH_WINDOW: BoolKey = BoolKey("sub-ass-scale-with-window");
pub const SUB_GAUSS: DoubleKey = DoubleKey("sub-gauss");
pub const SUB_GRAY: BoolKey = BoolKey("sub-gray");
pub const SUB_FILTER_SDH: BoolKey = BoolKey("sub-filter-sdh");
pub const SUB_FILTER_SDH_HARDER: BoolKey = BoolKey("sub-filter-sdh-harder");
pub const SECONDARY_SUBTITLES: BoolKey = BoolKey("secondary-subtitles");

// Playback
pub const RESUME_PLAYBACK: BoolKey = BoolKey("resume-playback");
pub const LOOP_MODE: StringKey = StringKey("loop-mode");
pub const KEEP_OPEN: StringKey = StringKey("keep-open");
pub const KEEP_OPEN_PAUSE: BoolKey = BoolKey("keep-open-pause");
pub const IDLE_MODE: StringKey = StringKey("idle-mode");

// Audio effects
pub const NOISE_STRENGTH: DoubleKey = DoubleKey("noise-strength");
pub const NOISE_GATE_THRESHOLD: IntKey = IntKey("noise-gate-threshold");
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

// HDR & tone mapping
pub const TONE_MAPPING: StringKey = StringKey("tone-mapping");
pub const TARGET_PEAK: IntKey = IntKey("target-peak");
pub const TARGET_CONTRAST: IntKey = IntKey("target-contrast");
pub const HDR_CONTRAST_RECOVERY: DoubleKey = DoubleKey("hdr-contrast-recovery");
pub const HDR_COMPUTE_PEAK: BoolKey = BoolKey("hdr-compute-peak");
pub const TARGET_COLORSPACE_HINT: BoolKey = BoolKey("target-colorspace-hint");
pub const GAMUT_MAPPING_MODE: StringKey = StringKey("gamut-mapping-mode");

// GLSL shaders
pub const GLSL_SHADERS: StringKey = StringKey("glsl-shaders");
pub const BUILTIN_UPSCALER: StringKey = StringKey("builtin-upscaler");
pub const BUILTIN_SHARPENER: StringKey = StringKey("builtin-sharpener");
pub const SHADER_FSR_SHARPNESS: DoubleKey = DoubleKey("shader-fsr-sharpness");
pub const SHADER_CAS_SHARPENING: DoubleKey = DoubleKey("shader-cas-sharpening");
pub const SHADER_NVSCALER_SHARPNESS: DoubleKey = DoubleKey("shader-nvscaler-sharpness");
pub const ANIME4K_PRESET: StringKey = StringKey("anime4k-preset");
pub const NEURAL_UPSCALER: StringKey = StringKey("neural-upscaler");

// Cache / buffer
pub const CACHE: BoolKey = BoolKey("cache");
pub const CACHE_SECS: IntKey = IntKey("cache-secs");
pub const CACHE_PAUSE: BoolKey = BoolKey("cache-pause");
pub const CACHE_ON_DISK: BoolKey = BoolKey("cache-on-disk");
pub const DEMUXER_READAHEAD_SECS: DoubleKey = DoubleKey("demuxer-readahead-secs");

// Screenshots
pub const SCREENSHOT_FORMAT: StringKey = StringKey("screenshot-format");
pub const SCREENSHOT_JPEG_QUALITY: IntKey = IntKey("screenshot-jpeg-quality");
pub const SCREENSHOT_PNG_COMPRESSION: IntKey = IntKey("screenshot-png-compression");
pub const SCREENSHOT_TEMPLATE: StringKey = StringKey("screenshot-template");
pub const SCREENSHOT_DIRECTORY: StringKey = StringKey("screenshot-directory");
pub const SCREENSHOT_TAG_COLORSPACE: BoolKey = BoolKey("screenshot-tag-colorspace");
pub const SCREENSHOT_HIGH_BIT_DEPTH: BoolKey = BoolKey("screenshot-high-bit-depth");

// GPU / advanced
pub const CUSTOM_MPV_OPTIONS: StringKey = StringKey("custom-mpv-options");
pub const GPU_DUMB_MODE: BoolKey = BoolKey("gpu-dumb-mode");
/// Video output mode: `"auto"` (try GL/offload, fall back to software when GL is
/// not viable — the default), `"gl"` (force OpenGL/offload), `"sw"` (force the
/// software/no-GL render path). Read at render setup; default `"auto"`.
pub const RENDER_MODE: StringKey = StringKey("render-mode");
pub const GPU_API: StringKey = StringKey("gpu-api");
pub const GPU_CONTEXT: StringKey = StringKey("gpu-context");

// ICC color management
pub const ICC_PROFILE: StringKey = StringKey("icc-profile");
pub const ICC_PROFILE_AUTO: BoolKey = BoolKey("icc-profile-auto");
pub const ICC_INTENT: IntKey = IntKey("icc-intent");

// Screensaver
pub const STOP_SCREENSAVER: StringKey = StringKey("stop-screensaver");

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
pub const CROSSFADE_SECS: IntKey = IntKey("crossfade-secs");
pub const AUDIO_VOLUME: IntKey = IntKey("audio-volume");

// Audio browser/layout
pub const BROWSER_PANE_WIDTH: IntKey = IntKey("browser-pane-width");
pub const BROWSER_MODE: IntKey = IntKey("browser-mode");
pub const BROWSER_VISIBLE: BoolKey = BoolKey("browser-visible");

// Audio background blur
pub const AUDIO_BG_BLUR: BoolKey = BoolKey("audio-bg-blur");

// High-contrast text mode
pub const AUDIO_HIGH_CONTRAST: BoolKey = BoolKey("audio-high-contrast");

// Sleep timer action
pub const SLEEP_TIMER_ACTION: StringKey = StringKey("sleep-timer-action");
pub const SLEEP_TIMER_MINUTES: IntKey = IntKey("sleep-timer-minutes");

// Library
pub const LIBRARY_GROUPING: StringKey = StringKey("library-grouping");
pub const LIBRARY_COLUMN_WIDTHS: StringKey = StringKey("library-column-widths");
pub const PLAYLIST_COLUMN_WIDTHS: StringKey = StringKey("playlist-column-widths");

// Resume playback (audio)
pub const QUEUE_CURRENT_INDEX: IntKey = IntKey("queue-current-index");
pub const PODCAST_ACTIVE_FEED: StringKey = StringKey("podcast-active-feed");

// Desktop integration
pub const TASKBAR_PROGRESS: BoolKey = BoolKey("taskbar-progress");

/// Use the current cover as a blurred background behind the audio controls.
pub const AUDIO_ALBUM_ART_BG: BoolKey = BoolKey("audio-album-art-bg");

// File browser
pub const FILES_LAST_DIR: StringKey = StringKey("files-last-dir");
