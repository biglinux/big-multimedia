use super::*;

use std::sync::atomic::{AtomicU64, Ordering};

use super::trim::{TrimExportOutcome, VideoTrimBarOutput};

#[derive(Debug)]
pub enum VideoShellInput {
    /// An mpv backend event (position, duration, pause, EOF, …).
    Player(PlayerEvent),
    /// An intent emitted by the OSD controls bar.
    Controls(VideoControlsBarOutput),
    /// An intent emitted by the folder-browser sidebar.
    Playlist(VideoBrowserSidebarOutput),
    /// An intent emitted by the IPTV browser (play a channel URL).
    Iptv(VideoIptvBrowserOutput),
    /// An intent emitted by the cast device picker (started / stopped).
    #[cfg(feature = "chromecast")]
    Cast(VideoCastDialogOutput),
    /// Open the IPTV browser dialog (header button).
    ShowIptvBrowser,
    /// Open the cast device picker dialog (header button).
    #[cfg(feature = "chromecast")]
    ShowCastDialog,
    /// Open the video filters/shaders dialog (header button).
    ShowFiltersDialog,
    /// Open the audio equalizer dialog (Media menu).
    ShowEqualizer,
    /// Open the audio & subtitle delay dialog (Media menu).
    ShowDelay,
    /// Open the image & video adjustment dialog (Media menu).
    ShowImageAdjust,
    /// Open yt-dlp stream-quality selector (Media menu).
    ShowYtdlQuality,
    /// Open the live media-information dialog (header button).
    ShowMediaInfo,
    /// Open the keyboard-shortcuts dialog (header button).
    ShowKeyboardShortcuts,
    /// Present the open-file picker (Open menu / welcome screen).
    ShowOpenFile,
    /// Present the open-URL entry dialog (Open menu / welcome screen).
    ShowOpenUrl,
    /// Present the recent-files chooser (Open menu / welcome screen).
    ShowRecent,
    /// Present the open-disc entry dialog (Open menu).
    ShowOpenDisc,
    /// Toggle the Files sidebar, reachable without the auto-hidden OSD header.
    ToggleFilesSidebar,
    /// Toggle the Playlist sidebar, reachable without the auto-hidden OSD header.
    TogglePlaylistSidebar,
    /// Open the categorized preferences window (Settings menu).
    ShowPreferences,
    /// Open the appearance (palette + layout) dialog (Settings menu).
    ShowAppearance,
    /// Confirm + reset all settings to defaults (Settings menu).
    ShowRestoreDefaults,
    /// Restore-defaults confirmed in the alert dialog: reset the store.
    RestoreDefaultsConfirmed,
    /// Files opened from the desktop / CLI / picker / drop: replace the
    /// playlist with these paths and start playing the first.
    OpenFiles(Vec<String>),
    /// Internal: an `OpenFiles` request whose folder-scan / playlist-file parse
    /// has been resolved off the main thread. Carries the final playlist, the
    /// index to start at, and the optional browse folder. Dispatched by
    /// `open_files` so the blocking disk I/O never runs on the GTK main loop.
    PlaylistResolved {
        entries: Vec<String>,
        start: usize,
        folder: Option<String>,
    },
    /// Open the go-to-time entry dialog (Playback menu).
    ShowGotoTime,
    /// Capture the current frame to the Pictures dir (Playback menu).
    Screenshot,
    ScreenshotQueueFailed,
    /// Open the chapter list dialog (Playback menu).
    ShowChapters,
    /// Jump to chapter `idx` (chapter list dialog).
    SetChapter(i64),
    /// Open the sleep-timer dialog (Playback menu).
    ShowSleepTimer,
    /// Arm the sleep timer for `minutes`, running `action` on expiry.
    SleepTimerStart {
        minutes: u32,
        action: String,
    },
    /// Disarm the sleep timer.
    SleepTimerCancel,
    /// One sleep-timer countdown tick (1 s).
    SleepTick,
    /// Toggle play/pause on the live mpv backend (right-click context menu /
    /// keyboard). No-op before mpv is up.
    TogglePlayback,
    /// Toggle the window's fullscreen state (double-click on the video /
    /// right-click context menu / fullscreen button).
    ToggleFullscreen,
    /// Activity transition from a pane host such as the shell desktop.
    SetPaneActivity(big_relm4_components::pane::PaneActivity),
    /// Extract the current file's audio to `codec` (mp3/m4a/flac) beside it —
    /// runs off the main loop via the in-process quick-export engine.
    ExtractAudio(String),
    /// Re-encode the current file into `container` (mp4/mkv/webm) beside it —
    /// the simple one-file convert; runs off the main loop.
    QuickConvert(String),
    /// Enter the interactive trim mode (timeline + multi-cut export).
    EnterTrimMode,
    /// Leave the interactive trim mode.
    ExitTrimMode,
    /// A typed effect emitted by the trim bar (seek preview / export / exit).
    TrimBar(VideoTrimBarOutput),
    /// Cancel the in-flight clip export (slim progress strip's Cancel button).
    TrimCancelExport,
    /// Mark a cut point at the live playback position (keyboard `M`). Two marks
    /// (in any order) make a cut.
    TrimMark,
    /// A clip export finished (success / cancellation / failure).
    TrimExportFinished(TrimExportOutcome),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ShellInputToken(u64);

static NEXT_SHELL_INPUT_TOKEN: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// Main-thread handle to the running shell's input, set at init so the
    /// application `open` handler can deliver opened files. The inputs carry
    /// GTK objects (not `Send`), so a thread-local `Sender` is used instead of
    /// a `MessageBroker` static. Public API; dead until the V6 cutover routes
    /// `app.rs::open` through it (mirrors the audio/converter ports).
    static SHELL_INPUT: RefCell<Option<(ShellInputToken, relm4::Sender<VideoShellInput>)>> =
        const { RefCell::new(None) };
}

/// Deliver a message to the running shell from the main thread (used by the
/// application `open` handler). No-op if the shell is not running yet.
pub fn send_to_shell(message: VideoShellInput) {
    SHELL_INPUT.with(|cell| {
        if let Some((_, sender)) = cell.borrow().as_ref() {
            let _ = sender.send(message);
        }
    });
}

pub(super) fn set_shell_input(sender: relm4::Sender<VideoShellInput>) -> ShellInputToken {
    let token = ShellInputToken(NEXT_SHELL_INPUT_TOKEN.fetch_add(1, Ordering::Relaxed));
    SHELL_INPUT.with(|cell| {
        *cell.borrow_mut() = Some((token, sender));
    });
    token
}

pub(super) fn clear_shell_input(token: ShellInputToken) {
    SHELL_INPUT.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().is_some_and(|(current, _)| *current == token) {
            slot.take();
        }
    });
}

pub(super) fn shell_input_sender() -> Option<relm4::Sender<VideoShellInput>> {
    SHELL_INPUT.with(|cell| cell.borrow().as_ref().map(|(_, sender)| sender.clone()))
}
