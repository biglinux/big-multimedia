//! App-local Relm4 glue for the video player (in-progress ground-up port).
//!
//! Reusable media widgets live in `big-media-components` / `big-relm4-components`
//! and engine-free player panels in `big-player-ui`; only genuinely
//! video-specific wiring (the `VideoShell` root, IPTV browser, cast overlays,
//! the GL video surface, filter/shader dialogs) lands here. Populated in V1+.

/// `glGetIntegerv` access for the GLArea fallback render path.
pub mod gl_framebuffer;

/// Relm4 root component for the video player (port in progress).
pub mod video_shell;

/// OSD controls bar (Relm4, app-local — recovers the old `controls_box`).
pub mod video_controls_bar;

/// Folder-browser playlist sidebar (Relm4, folder-scan-backed).
pub mod video_browser_sidebar;

/// IPTV browser (Relm4, video-local): providers, channels, logos, favorites.
pub mod iptv;

/// Cast (Chromecast) device picker dialog (Relm4, drives the shared engine).
#[cfg(feature = "chromecast")]
pub mod cast;

/// Video filters/shaders dialog (Relm4, mutates the live mpv filter chain).
pub mod filters;

/// Live media-information dialog (mpv-driven copyable rows).
pub mod media_info_dialog;

/// Keyboard-shortcuts dialog launcher (reuses the shared shortcut dialog).
pub mod keyboard_shortcuts_dialog;

/// Small hamburger-menu dialogs/actions: go-to-time, screenshot, chapters,
/// sleep timer (ported from the old god-object).
pub mod menu_dialogs;

/// Media-adjustment dialogs: equalizer (shared), audio/subtitle delay, and
/// image & video sliders (ported from the old god-object).
pub mod media_adjust;

/// Categorized preferences window (Video / Audio / Subtitles / General) ported
/// from the old god-object's `preferences/**` — switch/spin/dropdown rows bound
/// to settings keys with live mpv application.
pub mod preferences;

/// Appearance dialog: OSD palette + layout pages, button-layout editor with a
/// live miniature preview, and the app-owned OSD-theme CSS application.
pub mod appearance;
