use super::*;
use crate::shared::app_base::ShellInputSlot;

/// Messages into the shell. Almost all are child-panel `Output`s forwarded by
/// the Controllers' `forward` maps.
#[derive(Debug)]
pub enum AudioShellInput {
    /// The library database opened and the saved queue was read, or the
    /// database could not be opened for the reason given.
    QueueRestored(Result<super::queue_persistence::RestoredQueue, String>),
    RetryQueueSave,
    /// The library id (0 for an external file) and path of each file deleted or
    /// trashed off the main loop.
    TracksDeletedFromStorage(Vec<(i64, String)>),
    /// An intent emitted by the library panel.
    Library(LibraryPanelOutput),
    /// An intent emitted by the queue panel.
    Playlist(PlaylistPanelOutput),
    /// An mpv backend event (position, EOF, pause, …) from the player loop.
    Player(PlayerEvent),
    /// An intent emitted by the playback bar (transport controls).
    Playback(AudioPlaybackBarOutput),
    /// The noise reduction's model, mono or voice-only setting changed.
    NoiseSettingsChanged,
    /// An intent emitted by the visual trim bar.
    Trim(AudioTrimBarOutput),
    /// An intent emitted by the files browser.
    Files(FileBrowserOutput),
    /// An intent emitted by the radio panel.
    Radio(RadioPanelOutput),
    /// Recording directory prepared off the GTK thread; stale requests are ignored.
    RadioRecordingPrepared {
        generation: u64,
        source: String,
        path: PathBuf,
        result: Result<(), String>,
    },
    /// An intent emitted by the lyrics panel (expand toggle).
    Lyrics(LyricsPanelOutput),
    /// Lyrics resolved for a track (async fetch); `gen` guards against a stale
    /// result from a track the user already skipped past.
    LyricsResolved {
        /// Track generation this fetch was started for.
        r#gen: u64,
        /// Plain-text lyrics, or `None` if not found.
        text: Option<String>,
    },
    /// An intent emitted by the podcast panel (load feed / notify).
    Podcast(PodcastPanelOutput),
    /// A podcast feed fetched (async) → enter podcast mode, queue episodes, play.
    PodcastFeedResolved {
        /// Feed title (shown in the queue header).
        title: String,
        /// `(audio_url, episode_title)` per episode, in feed order.
        episodes: Vec<(String, String)>,
    },
    /// The visible view-stack tab changed → enter/leave podcast mode.
    ViewTabChanged(String),
    /// Saved radio stations resolved (async DB result) → push to the radio panel.
    RadioStationsResolved(Vec<RadioStation>),
    /// A folder scan finished → rebuild the library tree + playlist dialog data.
    ScanFinished(bool),
    /// A bounded, cancellable import finished (earlier commits are preserved).
    LibraryScanFinished {
        control: Arc<library_scan_engine::ScanControl>,
        report: library_scan_engine::ScanReport,
    },
    /// Dropped/opened paths resolved to library tracks (async DB result) →
    /// insert into the queue at `insert_pos`.
    DroppedResolved {
        /// Queue index to insert at.
        insert_pos: usize,
        /// Resolved tracks (paths not in the library are dropped).
        tracks: Vec<Track>,
    },
    /// A browse-tree node resolved to its tracks (async DB result) → append to
    /// the queue and start playback when idle.
    NodeTracksResolved(Vec<Track>),
    /// Rebuild the library tree from the current grouping/filter/age (boot +
    /// after a grouping/search/age change).
    RebuildLibraryTree,
    LibraryTreeFailed {
        generation: u64,
    },
    /// The rebuilt library tree (async DB result) → push to the library panel.
    LibraryTreeResolved {
        /// Current search/grouping/age generation.
        generation: u64,
        /// Snapshot built off the main thread.
        data: LibraryTreeSnapshot,
        /// The library holds tracks, whatever the search and age filters hide.
        has_items: bool,
    },
    /// A playlist mutation completed (false leaves the current view unchanged).
    PlaylistMutationFinished(bool),
    /// The playlist picker could not read its data.
    PlaylistReadFailed,
    /// Reload playlist dialog data after any playlist DB change.
    RefreshPlaylists,
    /// Edited tags were written to file + DB → patch the queue + rebuild tree.
    TagsWritten {
        /// Track DB id.
        track_id: i64,
        /// The edited field values (to patch in-memory queue rows).
        fields: TagEditFields,
    },
    /// MusicBrainz metadata resolved for the currently open tag editor.
    MusicBrainzLookupResolved {
        /// Track DB id the lookup was requested for.
        track_id: i64,
        /// Returned MusicBrainz fields, or `None` when no confident match exists.
        metadata: Option<crate::library::musicbrainz::MbMetadata>,
    },
    /// Imported playlist file parsed to paths → resolve + append to the queue.
    ImportPaths(Vec<String>),
    /// Files opened from the desktop / CLI (app `open` handler): expand
    /// playlists, append as external tracks, and play from the first.
    OpenPaths(Vec<String>),
    OpenPathsResolved {
        generation: u64,
        paths: Vec<String>,
        failed: bool,
        /// A playlist among them was cut at its entry cap.
        truncated: bool,
    },
    ExternalMetadataResolved(Vec<Track>),
    /// Open the equalizer dialog (main menu).
    ShowEqualizer,
    /// Resolve watched music folders, then open the folder manager.
    ShowManageFolders,
    /// Watched music folders resolved → push into the library dialog.
    ManageFoldersResolved(Vec<WatchFolder>),
    /// Open the "Play on a TV" picker (main menu).
    ShowCast,
    /// BigNetScreen reported a change (session, position, receivers).
    TvChanged,
    /// A button of the TV strip.
    Tv(crate::shared::tv_cast::TvCardAction),
    /// Open the smartphone web-remote dialog (main menu).
    ShowSmartphone,
    /// Re-evaluate the album-art background (Appearance toggle changed).
    RefreshBlurArt,
    /// Open the playlist library dialog (main menu): resolves + presents.
    ShowManagePlaylists,
    /// Custom playlists resolved → push into the playlist library dialog.
    ManagePlaylistsResolved(Vec<(i64, String, i64, f64)>),
    /// An action intent from the playlist library dialog.
    PlaylistLibraryDialog(crate::audio::ui::playlist_library_dialog::PlaylistLibraryDialogOutput),
    /// A saved playlist's tracks resolved → replace the queue and play.
    PlaylistTracksForQueue(Vec<Track>),
    /// An imported playlist file was chosen → parse + save as a new playlist.
    ImportPlaylistFile {
        /// Absolute path to the `.m3u`/`.pls` file.
        path: PathBuf,
        /// Suggested playlist name (file stem).
        name: String,
    },
    /// A folder was chosen for batch import → parse every playlist file in it.
    ImportPlaylistFolder(PathBuf),
    /// Import finished → `usize` tracks were matched and added.
    /// Tracks saved, and whether a playlist was cut at its entry cap.
    PlaylistImported(Result<(usize, bool), String>),
    /// A transport command from the smartphone remote (worker thread); reuses
    /// the normal playback handler.
    PlaybackRemote {
        command: AudioPlaybackBarOutput,
        permit: crate::audio::ui::smartphone_dialog::RemoteActionPermit,
    },
    /// Open the sleep-timer dialog (main menu).
    ShowSleepTimer,
    /// Open preferences bound to this shell's actual playback engine.
    ShowPreferences,
    /// Apply the persisted taskbar-progress choice without a restart.
    TaskbarProgressChanged(bool),
    /// Show or remove the tray icon, as Preferences now says.
    TrayIconChanged(bool),
    /// The Maximum Volume preference changed: the volume slider follows it.
    VolumeMaxChanged,
    /// A click on the tray icon or its menu.
    Tray(big_app_kit::tray::TrayCommand),
    /// Arm the sleep timer for `minutes`, then perform `action` on expiry.
    SleepTimerStart {
        /// Countdown length in minutes.
        minutes: u32,
        /// `"pause"` | `"quit"` | `"suspend"` | `"poweroff"`.
        action: String,
    },
    /// Cancel an armed sleep timer.
    SleepTimerCancel,
    /// One-second sleep-timer countdown tick.
    SleepTick,
    /// Tell the person something. Used by workers that must report a failure
    /// without leaving the previous result on screen as if it had succeeded.
    Notify(String),
    /// Undo the last queue edit (Ctrl+Z).
    Undo,
    /// Redo the last undone queue edit (Ctrl+Shift+Z / Ctrl+Y).
    Redo,
    /// Toggle shuffle from a keyboard/action shortcut.
    ToggleShuffleShortcut,
    /// Keyboard: scroll the queue to the playing track.
    ScrollToPlaying,
    /// Keyboard: select every track of the queue.
    SelectAllInQueue,
    /// Keyboard seek by this many seconds from the current position.
    SeekBy(f64),
    /// Typed MPRIS command, with track identity preserved across thread dispatch.
    MprisCommand(MprisCommand),
    /// Album art resolved, decoded and cached for MPRIS off the main thread;
    /// `gen` guards against a stale result → blur bg, card and `artUrl`.
    ArtResolved {
        /// Track generation this fetch was started for.
        r#gen: u64,
        /// The track's art, or `None` when it has none.
        art: Option<super::metadata::ResolvedArt>,
    },
    /// Existing playlists resolved for the add-to-playlist picker (async DB
    /// result) → open the picker in the library panel.
    AddToPlaylistResolved {
        /// Track to add.
        track: Track,
        /// `(id, name)` of existing playlists.
        playlists: Vec<(i64, String)>,
    },
    /// Open the current track in the audio converter.
    ConvertTrack,
    /// Enter visual Trim & Export mode for the current track.
    ShowTrimExport,
}

thread_local! {
    /// The inputs carry GTK objects (not `Send`), so the slot is thread-local
    /// instead of a `MessageBroker` static.
    pub(super) static SHELL_INPUT: ShellInputSlot<relm4::Sender<AudioShellInput>> =
        const { ShellInputSlot::new() };
}

/// Deliver a message to the open shell from the main thread (used by the
/// application `open` handler). No-op when no shell is open.
pub fn send_to_shell(message: AudioShellInput) {
    if let Some(sender) = SHELL_INPUT.with(ShellInputSlot::sender) {
        let _ = sender.send(message);
    }
}
