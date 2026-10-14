//! Relm4 root component for the video player (`lib.rs::run` launches it).
//!
//! Wires the mpv backend, the GL/DMA-BUF video surface (shared `video_surface`
//! plumbing: offload with GLArea fallback), and the OSD controls bar
//! (`video_controls_bar`). Push-model: the controls emit intents → the shell
//! drives mpv; mpv events flow back into the controls display. The playlist
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

use crate::i18n::{i18n, ni18n};
use crate::player::mpv_backend::fast_render::{
    FastRenderContext, SwRenderContext as SoftwareRenderContext,
};
use crate::player::mpv_backend::offload_renderer::VideoBackend;
use crate::player::mpv_backend::{MpvBackend, PlayerEvent, video_surface};
use crate::settings::{Settings, keys};
use crate::shared::converter_handoff;
use crate::video::ui::filters::{VideoFiltersDialog, VideoFiltersDialogInit};
use crate::video::ui::gl_framebuffer::{GL_FRAMEBUFFER_BINDING, get_gl_get_integerv};
use crate::video::ui::iptv::{VideoIptvBrowser, VideoIptvBrowserOutput};
use crate::video::ui::keyboard_shortcuts_dialog::show_video_keyboard_shortcuts_dialog;
use crate::video::ui::media_adjust;
use crate::video::ui::media_details_dialog::show_video_media_details_dialog;
use crate::video::ui::menu_dialogs::sleep_minutes_to_secs;
use crate::video::ui::video_browser_sidebar::{
    SidebarMode, VideoBrowserSidebar, VideoBrowserSidebarInput, VideoBrowserSidebarOutput,
};
use crate::video::ui::video_controls_bar::{
    VideoControlsBar, VideoControlsBarInit, VideoControlsBarInput, VideoControlsBarOutput,
};
use big_app_kit::file_dialogs::FilePicker;
use big_media_components::media::files::VIDEO_EXTENSIONS;
use big_media_components::time::playback_time;
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};

/// OSD auto-hide delay after the last pointer activity.
const OSD_HIDE_DELAY_MS: u64 = 2500;

/// Pointer travel (px) before a button-1 drag over the video begins a window
/// move.
const DRAG_THRESHOLD: f64 = 8.0;

/// OSD reveal/auto-hide state machine, shared (interior-mutable) between the
/// motion controllers and `update()`. Reveals the header + controls on pointer
/// motion over the video, hides them after [`OSD_HIDE_DELAY_MS`] of inactivity
/// or on pointer-leave.
mod actions;
mod adjustments;
mod component;
mod dialogs;
mod export;
mod idle_wallpaper;
mod input;
mod input_gestures;
mod keyboard;
mod menus;
mod mpris;
mod osd;
mod panel;
mod playback;
pub(crate) use playback::is_remote_uri;
mod playlist_panel;
mod render;
mod state_access;
mod trim;
mod tv;

pub use input::{VideoShellInput, mark_opening_files, send_to_shell};

use osd::{AspectSnap, IdleBackground, OsdState};

pub struct VideoShell {
    /// OSD controls bar (seek row + volume/speed/voice + transport + tracks).
    controls: Controller<VideoControlsBar>,
    /// Folder-browser playlist sidebar (folder scan = the playlist).
    playlist: Controller<VideoBrowserSidebar>,
    /// The playlist and folder browser, floating over the video. Kept in the
    /// model so keyboard/GAction routes can reveal Files/Playlist even while
    /// the OSD header is auto-hidden.
    playlist_panel: playlist_panel::PlaylistPanel,
    /// Last sidebar mode requested by a button, keyboard shortcut, or GAction.
    sidebar_mode: Rc<Cell<SidebarMode>>,
    /// Header Files toggle, mirrored when keyboard/GAction opens the sidebar.
    files_sidebar_button: gtk::ToggleButton,
    /// Header Playlist toggle, mirrored when keyboard/GAction opens the sidebar.
    playlist_sidebar_button: gtk::ToggleButton,
    /// Guard shared with the header toggles to avoid re-entrant state changes.
    sidebar_toggle_updating: Rc<Cell<bool>>,
    /// IPTV browser (providers / channels / favorites). Presented in a dialog
    /// from the header button; emits play intents routed to mpv.
    iptv: Controller<VideoIptvBrowser>,
    /// Main video overlay; owns the welcome screen child (added/removed).
    main_overlay: gtk::Overlay,
    /// Centered welcome screen shown when nothing is loaded; `None` once hidden.
    welcome_overlay: RefCell<Option<gtk::Box>>,
    /// Paths of the mpv playlist, in load order. Kept to detect when a sidebar
    /// `PlayFromList` reuses the already-loaded list (jump in place) versus a new
    /// folder (reload), and to resolve a `RemoveEntry` path to its mpv index.
    playlist_paths: Vec<String>,
    /// Pending load-time shuffle: the chosen start path and the list length,
    /// applied at the `FileLoaded` where mpv holds both.
    shuffle_on_load: Option<(String, usize)>,
    /// What is away from normal, and the header pill that says so.
    adjustments: Vec<adjustments::Adjustment>,
    adjustments_pill: gtk::Button,
    /// Header pill offering HDR for a picture that plays grey (this file).
    hdr_hint: gtk::Button,
    /// What "Back to normal" just undid, for the panel's "Undo".
    last_reset: Option<Vec<adjustments::Adjustment>>,
    /// The settings panel (right-hand card over the video).
    panel: Rc<panel::SettingsPanel>,
    /// This player's line to BigNetScreen: TV page, card and controls.
    tv: Rc<crate::shared::tv_cast::TvLink>,
    /// Over the video while a TV plays this player's session.
    tv_card: crate::shared::tv_cast::TvCard,
    /// Header button while the TV plays but the player was released; opens
    /// `tv_menu`.
    tv_pill: gtk::MenuButton,
    /// What the TV plays and the ways back, under `tv_pill`.
    tv_menu: crate::shared::tv_cast::TvCard,
    /// "Play the TV's here": seek to the TV's point once its file loads.
    tv_catch_up: bool,
    /// The session's last state: where it was when it ended.
    tv_last: Option<crate::cast::daemon::State>,
    /// Whether the session was released at the last change: a released
    /// session ending leaves the player where it is.
    tv_was_released: bool,
    /// Next/Previous or the end of a file: the TV that the next file to load
    /// goes to.
    tv_follow: Option<String>,
    /// "Watch here too": the player plays in step with the TV, muted; holds
    /// the mute to give back.
    tv_here: Option<bool>,
    /// The last whole second told to the playlist (its end time).
    playlist_second: i64,
    /// Video playback backend (system libmpv). `None` until the surface
    /// realizes and the backend initializes; `None` also if init failed.
    mpv: Rc<RefCell<Option<Arc<MpvBackend>>>>,
    /// The GL render path, built when the first media loads.
    video_start: render::VideoStart,
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
    /// Messages for the person, in a row above the content.
    notice: big_media_components::surface::MediaNotice,
    screenshot_owner: Arc<std::sync::atomic::AtomicBool>,
    /// The `win.*` action group, kept so the shell can gate context-dependent
    /// entries (e.g. enable Stream Quality only for online media).
    win_actions: gio::SimpleActionGroup,
    /// Armed sleep-timer countdown source; `None` when disarmed. Stored so it is
    /// removed on disarm / re-arm / drop (no leaked `glib::timeout`).
    sleep_timer_source: Option<glib::SourceId>,
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
    /// "Play only the marked cuts" toggle state.
    trim_play_selection: Rc<Cell<bool>>,
    /// Sorted `(start, end)` cut ranges; drives play-selection looping.
    trim_ranges: Rc<RefCell<Vec<(f64, f64)>>>,
    /// True while interactive trim mode is active. Shared (`Rc`) with the OSD pin
    /// and the trim key controller.
    trim_active: Rc<Cell<bool>>,
    /// MPRIS2 service; `None` in background mode or without a session bus.
    mpris: Option<big_app_kit::mpris::Mpris>,
    /// Snapshot the MPRIS service polls, mirrored from mpv events.
    mpris_state: Arc<std::sync::Mutex<big_app_kit::mpris::PlayerState>>,
    /// Counter behind each loaded file's MPRIS track id.
    mpris_track_seq: u64,
    /// True between an unpause and the next pause or end of file.
    playing: bool,
    /// GTK inhibit cookie keeping the screen awake. mpv cannot do it: its
    /// screensaver control needs a window of its own, which libmpv lacks.
    screen_inhibit: Option<u32>,
}
