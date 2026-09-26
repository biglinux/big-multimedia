use super::*;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::trim::VideoTrimBarOutput;

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
    /// Open the IPTV browser dialog (header button).
    ShowIptvBrowser,
    /// BigNetScreen reported a change (session, position, receivers).
    TvChanged,
    /// The TV card's "Stop sending".
    TvStop,
    /// "Use the player" (the TV plays on without the controls), or "Control
    /// the TV" once released.
    TvToggleRelease,
    /// "Watch here too" (the player plays in step with the TV, silent), or
    /// "Only on the TV".
    TvHere,
    /// "Show this one on the TV": the TV plays what the player holds.
    TvReplace,
    /// "Play the TV's here": the player opens what the TV plays, in step.
    TvFollow,
    /// The header's settings button: open the panel home, or close it.
    TogglePanel,
    /// A shortcut, chip or menu entry: open the panel straight at a section
    /// (Esc then closes the panel).
    ShowPanel(super::panel::PanelPage),
    /// A block or link inside the panel: switch to that page.
    PanelNavigate(super::panel::PanelPage),
    /// The panel's Back button.
    PanelBack,
    /// Esc inside the panel.
    PanelEscape,
    /// Close the settings panel.
    ClosePanel,
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
    /// The preferences dialog closed: re-read settings the shell applies
    /// itself (the screen-awake inhibitor).
    PreferencesClosed,
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
    /// Jump to chapter `idx` (chapter list dialog).
    SetChapter(i64),
    /// Arm the sleep timer for `minutes`, running `action` on expiry.
    SleepTimerStart {
        minutes: u32,
        action: String,
    },
    /// Disarm the sleep timer.
    SleepTimerCancel,
    /// Re-read which settings are away from normal (controls shown, a
    /// dialog closed, speed changed).
    RefreshAdjustments,
    /// A chip's × or a notice's button: put these back to normal, with an
    /// Undo in the settings panel.
    ResetAdjustments(Vec<super::adjustments::Adjustment>),
    /// "Back to normal": every chip but the sleep timer at once.
    ResetAllAdjustments,
    /// A second after `path` loaded: offer HDR when it looks like
    /// undescribed HDR.
    SuggestHdr(String),
    /// The panel's Undo: put these settings back as they were.
    RestoreAdjustments(Vec<super::adjustments::Adjustment>),
    /// One sleep-timer countdown tick (1 s).
    SleepTick,
    /// Toggle play/pause on the live mpv backend (right-click context menu /
    /// keyboard). No-op before mpv is up.
    TogglePlayback,
    /// Toggle the window's fullscreen state (double-click on the video /
    /// right-click context menu / fullscreen button).
    ToggleFullscreen,
    /// Open the current file in the audio converter to keep only its audio.
    ExtractAudio,
    /// Open the current file in the video converter.
    Convert,
    /// Enter the interactive trim mode (mark cuts, hand them to the converter).
    EnterTrimMode,
    /// Leave the interactive trim mode.
    ExitTrimMode,
    /// A typed effect emitted by the trim bar (seek preview / export / exit).
    TrimBar(VideoTrimBarOutput),
    /// Mark a cut point at the live playback position (keyboard `M`). Two marks
    /// (in any order) make a cut.
    TrimMark,
    /// A command from the MPRIS2 D-Bus service (media keys, desktop widgets).
    Mpris(big_app_kit::mpris::MprisCommand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ShellInputToken(u64);

static NEXT_SHELL_INPUT_TOKEN: AtomicU64 = AtomicU64::new(1);

/// Set when the process starts in order to open files, read once by the
/// shell's init: it then starts on the video area instead of the welcome
/// screen and wallpaper, which stood for about a second before the first frame.
static OPENING_FILES: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Main-thread handle to the running shell's input, set at init so the
    /// application `open` handler can deliver opened files. The inputs carry
    /// GTK objects (not `Send`), so a thread-local `Sender` is used instead of
    /// a `MessageBroker` static.
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

/// Tell the shell about to be created that files are on their way. A shell
/// already running keeps its screen: its files replace the playing ones.
pub fn mark_opening_files() {
    if SHELL_INPUT.with(|cell| cell.borrow().is_none()) {
        OPENING_FILES.store(true, Ordering::Relaxed);
    }
}

pub(super) fn take_opening_files() -> bool {
    OPENING_FILES.swap(false, Ordering::Relaxed)
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
