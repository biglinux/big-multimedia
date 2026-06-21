//! Relm4 root component for the video player — the live UI (`lib.rs::run`
//! launches this; the legacy god-object was deleted in the V6 cutover).
//!
//! Mirrors `big_audio_player::audio::ui::audio_shell::AudioShell`. Wires the
//! mpv backend, the GL/DMA-BUF video surface (shared `video_surface` plumbing —
//! offload with GLArea fallback), and the OSD `VideoPlaybackBar` (the shared
//! `BigVideoTransport`). Push-model: the transport emits intents → the shell
//! drives mpv; mpv events flow back into the transport display. The playlist
//! sidebar, IPTV browser, cast picker, and filter/info/shortcut dialogs hang
//! off the same shell.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use log::error;
use relm4::{Component, ComponentController, ComponentParts, ComponentSender, Controller};

#[cfg(feature = "chromecast")]
use crate::cast::chromecast::CastManager;
use crate::i18n::{i18n, ni18n};
use crate::player::mpv_backend::fast_render::{FastRenderContext, SwRenderContext};
use crate::player::mpv_backend::offload_renderer::VideoBackend;
use crate::player::mpv_backend::{skip_mpv_render, video_surface, MpvBackend, PlayerEvent};
use crate::settings::{keys, Settings};
#[cfg(feature = "chromecast")]
use crate::video::ui::cast::{VideoCastDialog, VideoCastDialogInit, VideoCastDialogOutput};
use crate::video::ui::filters::{VideoFiltersDialog, VideoFiltersDialogInit};
use crate::video::ui::gl_framebuffer::{get_gl_get_integerv, GL_FRAMEBUFFER_BINDING};
use crate::video::ui::iptv::{VideoIptvBrowser, VideoIptvBrowserOutput};
use crate::video::ui::keyboard_shortcuts_dialog::show_video_keyboard_shortcuts_dialog;
use crate::video::ui::media_adjust;
use crate::video::ui::media_info_dialog::show_video_media_info_dialog;
use crate::video::ui::menu_dialogs::sleep_minutes_to_secs;
use crate::video::ui::video_browser_sidebar::{
    SidebarMode, VideoBrowserSidebar, VideoBrowserSidebarInput, VideoBrowserSidebarOutput,
};
use crate::video::ui::video_controls_bar::{
    VideoControlsBar, VideoControlsBarInit, VideoControlsBarInput, VideoControlsBarOutput,
};
use big_app_kit::file_dialogs::FilePicker;
use big_media_components::converter::progress::BigConverterProgress;
use big_media_components::media::files::VIDEO_EXTENSIONS;
use big_media_components::time::playback_time;
use big_media_runtime::converter::queue::ConversionQueue;
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};

/// OSD auto-hide delay after the last pointer activity (mirrors the old
/// `HIDE_DELAY_MS`).
const OSD_HIDE_DELAY_MS: u64 = 2500;

/// Tick interval for the animated idle gradient (mirrors the old 33 ms tick).
const IDLE_BG_TICK_MS: u64 = 33;

/// Pointer travel (px) before a button-1 drag over the video begins a window
/// move (mirrors the old `input/mod.rs` `DRAG_THRESHOLD`).
const DRAG_THRESHOLD: f64 = 8.0;

/// How the shell is mounted. `Player` exposes the full application chrome;
/// `Background` keeps the same mpv/GL surface but omits player UI overlays so
/// the content can behave like a desktop video wallpaper.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VideoShellMode {
    #[default]
    Player,
    Background,
}

impl VideoShellMode {
    pub fn is_background(self) -> bool {
        matches!(self, Self::Background)
    }
}

/// OSD reveal/auto-hide state machine, shared (interior-mutable) between the
/// motion controllers and `update()`. Reveals the header + controls on pointer
/// motion over the video, hides them after [`OSD_HIDE_DELAY_MS`] of inactivity
/// or on pointer-leave. Mirrors the old `controls/visibility.rs`.
mod actions;
#[cfg(feature = "chromecast")]
mod cast_snapshot;
mod component;
mod dialogs;
mod export;
mod input;
mod input_gestures;
mod keyboard;
mod osd;
mod playback;
mod render;
mod state_access;
mod trim;

pub use input::{send_to_shell, VideoShellInput};

use osd::{AspectSnap, IdleBackground, OsdState};

pub struct VideoShell {
    /// Mount mode: full player UI or desktop-background video surface.
    mode: VideoShellMode,
    /// OSD controls bar (seek row + volume/speed/voice + transport + tracks).
    controls: Controller<VideoControlsBar>,
    /// Folder-browser playlist sidebar (folder scan = the playlist).
    playlist: Controller<VideoBrowserSidebar>,
    /// IPTV browser (providers / channels / favorites). Presented in a dialog
    /// from the header button; emits play intents routed to mpv.
    iptv: Controller<VideoIptvBrowser>,
    /// Cast (Chromecast) device picker. Presented in a dialog from the header
    /// button; drives the shared `CastManager` and reports start/stop here.
    #[cfg(feature = "chromecast")]
    cast: Controller<VideoCastDialog>,
    /// Main video overlay — owns the welcome screen child (added/removed).
    main_overlay: gtk::Overlay,
    /// Centered welcome screen shown when nothing is loaded; `None` once hidden.
    welcome_overlay: RefCell<Option<gtk::Box>>,
    /// Paths of the mpv playlist, in load order. Kept to detect when a sidebar
    /// `PlayFromList` reuses the already-loaded list (jump in place) versus a new
    /// folder (reload), and to resolve a `RemoveEntry` path to its mpv index.
    playlist_paths: Vec<String>,
    /// Video playback backend (system libmpv). `None` until the surface
    /// realizes and the backend initializes; `None` also if init failed.
    mpv: Rc<RefCell<Option<Arc<MpvBackend>>>>,
    /// Duration of the loaded media (scrub fraction → seconds conversion).
    duration_secs: f64,
    /// Player settings (passed to the on-demand filters/shortcuts dialogs).
    settings: Settings,
    /// Live transform state for values mpv does not expose back directly
    /// (currently flip/crop), shared with cast snapshot construction.
    video_transform: media_adjust::VideoTransformState,
    /// OSD reveal/auto-hide state (shared with the motion controllers).
    osd: Rc<OsdState>,
    /// Animated idle gradient behind the welcome screen (None-safe teardown).
    idle_bg: Rc<IdleBackground>,
    /// Centered buffering spinner overlay; shown on `Buffering(true)`.
    buffering_overlay: gtk::Box,
    /// Transient-toast host wrapping the window content (screenshot / chapter /
    /// sleep-timer feedback). Mirrors the god-object's `toast_overlay`.
    toast_overlay: adw::ToastOverlay,
    /// Armed sleep-timer countdown source; `None` when disarmed. Stored so it is
    /// removed on disarm / re-arm / drop (no leaked `glib::timeout`).
    sleep_timer_source: Option<glib::SourceId>,
    /// True when a pane-host activity transition paused playback. Used to
    /// resume only playback this host paused, not media the user paused.
    activity_paused_playback: Cell<bool>,
    /// Seconds left on the armed sleep timer (drives the volume fade + expiry).
    sleep_timer_remaining: u32,
    /// On-expiry action id (`pause` / `quit` / `suspend` / `poweroff`).
    sleep_timer_action: String,
    /// Pre-fade volume, restored when the timer fires (so the next play is not
    /// stuck at the faded-down level).
    sleep_timer_volume: f64,
    /// Window aspect-ratio snap (resize to fit the video, no letterbox bars).
    aspect_snap: Rc<AspectSnap>,
    /// Token for the thread-local shell input broker. Cleared on drop so a
    /// closed host module does not keep the Relm4 component alive.
    shell_input_token: input::ShellInputToken,
    /// Interactive trim bar (bottom overlay), shown only in trim mode.
    trim: Controller<trim::VideoTrimBar>,
    /// Shared conversion-progress page, shown as an overlay during clip export.
    trim_progress: Controller<BigConverterProgress>,
    /// Shared re-encode options (container/codec/quality) — same vocabulary as
    /// the full converter; shown in the re-encode settings dialog.
    trim_encode: big_converter_ui::encode_options::EncodeOptions,
    /// Colour adjustments applied on a precise export (set in the settings dialog).
    trim_filters: Rc<RefCell<big_media_runtime::converter::profiles::VideoFilters>>,
    /// Output layout: true = each cut to its own file, false = joined.
    trim_split: Rc<Cell<bool>>,
    /// Reused re-encode settings window (built lazily on the gear).
    trim_encode_dialog: Rc<RefCell<Option<adw::Window>>>,
    /// Full-cover overlay hosting the progress page while an export runs.
    trim_progress_overlay: gtk::Box,
    /// True while interactive trim mode is active. Shared (`Rc`) with the OSD pin
    /// and the trim key controller.
    trim_active: Rc<Cell<bool>>,
    /// Live clip-export queue (kept so the progress page can cancel it).
    trim_queue: Rc<RefCell<Option<ConversionQueue>>>,
}

#[cfg(test)]
mod tests {
    use super::{dialogs, VideoShellMode, DRAG_THRESHOLD, IDLE_BG_TICK_MS, OSD_HIDE_DELAY_MS};

    #[test]
    fn shell_interaction_timing_constants_stay_ordered() {
        let idle_tick_ms = std::hint::black_box(IDLE_BG_TICK_MS);
        let osd_hide_ms = std::hint::black_box(OSD_HIDE_DELAY_MS);
        let drag_threshold = std::hint::black_box(DRAG_THRESHOLD);

        assert!(idle_tick_ms < osd_hide_ms);
        assert!(drag_threshold > 0.0);
    }

    #[test]
    fn shell_background_mode_is_distinct_from_full_player() {
        assert!(!VideoShellMode::Player.is_background());
        assert!(VideoShellMode::Background.is_background());
    }

    #[test]
    fn shell_keeps_disc_uri_contract_available() {
        assert_eq!(
            dialogs::build_disc_uri("dvd", "/dev/sr0").as_deref(),
            Some("dvd://--dvd-device=/dev/sr0")
        );
        assert_eq!(
            dialogs::build_disc_uri("bd", "/dev/sr1").as_deref(),
            Some("bd://--bluray-device=/dev/sr1")
        );
        assert_eq!(dialogs::build_disc_uri("dvd", "--no-config"), None);
    }

    #[test]
    fn shell_disc_uri_rejects_empty_device() {
        assert_eq!(dialogs::build_disc_uri("dvd", ""), None);
        assert_eq!(dialogs::build_disc_uri("bd", ""), None);
    }

    #[test]
    fn shell_disc_uri_rejects_unknown_response() {
        assert_eq!(dialogs::build_disc_uri("cancel", "/dev/sr0"), None);
        assert_eq!(dialogs::build_disc_uri("", "/dev/sr0"), None);
    }
}
