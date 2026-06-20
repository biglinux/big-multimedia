use super::*;

/// Messages into the shell — almost all are child-panel `Output`s forwarded by
/// the Controllers' `forward` maps.
#[derive(Debug)]
pub enum AudioShellInput {
    /// Tracks whose files were deleted/trashed off the main loop landed; carries
    /// the DB ids that succeeded so the queue + database are reconciled on the
    /// main thread (by id, robust to any queue change during the deletes).
    TracksDeletedFromStorage(Vec<i64>),
    /// An intent emitted by the library panel.
    Library(LibraryPanelOutput),
    /// An intent emitted by the queue panel.
    Playlist(PlaylistPanelOutput),
    /// An mpv backend event (position, EOF, pause, …) from the player loop.
    Player(PlayerEvent),
    /// An intent emitted by the playback bar (transport controls).
    Playback(AudioPlaybackBarOutput),
    /// An intent emitted by the files browser.
    Files(FileBrowserOutput),
    /// An intent emitted by the radio panel.
    Radio(RadioPanelOutput),
    /// An intent emitted by the lyrics panel (expand toggle).
    Lyrics(LyricsPanelOutput),
    /// Lyrics resolved for a track (async fetch); `gen` guards against a stale
    /// result from a track the user already skipped past.
    LyricsResolved {
        /// Track generation this fetch was started for.
        gen: u64,
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
    /// A folder scan finished → rebuild the library tree + playlist-manager data.
    ScanFinished,
    /// Dropped/opened paths resolved to library tracks (async DB result) →
    /// insert into the queue at `insert_pos`.
    DroppedResolved {
        /// Queue index to insert at.
        insert_pos: usize,
        /// Resolved tracks (paths not in the library are dropped).
        tracks: Vec<Track>,
    },
    /// A smart/playlist node resolved to its tracks (async DB result) →
    /// populate the detail view.
    DetailResolved(Vec<Track>),
    /// A browse-tree node resolved to its tracks (async DB result) → append to
    /// the queue and start playback when idle (mirrors the god-object's
    /// activate-to-enqueue Music tab).
    NodeTracksResolved(Vec<Track>),
    /// Rebuild the library tree from the current grouping/filter/age (boot +
    /// after a grouping/search/age change).
    RebuildLibraryTree,
    /// The rebuilt library tree (async DB result) → push to the library panel.
    LibraryTreeResolved(LibraryTreeData),
    /// Reload playlist-manager data after any playlist DB change.
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
    /// Open the equalizer dialog (main menu).
    ShowEqualizer,
    /// Open the cast device-picker dialog (main menu).
    #[cfg(feature = "chromecast")]
    ShowCast,
    /// Cast the current track to the discovered device at this index.
    #[cfg(feature = "chromecast")]
    CastToDeviceIndex(usize),
    /// Stop casting to the current device.
    #[cfg(feature = "chromecast")]
    CastStop,
    /// Open the smartphone web-remote dialog (main menu).
    ShowSmartphone,
    /// Re-evaluate the album-art background (Appearance toggle changed).
    RefreshBlurArt,
    /// Open the playlist manager dialog (main menu) — resolves + presents.
    ShowManagePlaylists,
    /// Custom playlists resolved → push into the manager dialog.
    ManagePlaylistsResolved(Vec<(i64, String, i64, f64)>),
    /// An action intent from the playlist manager dialog.
    PlaylistManager(crate::audio::ui::playlist_manager::PlaylistManagerOutput),
    /// A saved playlist's tracks resolved → replace the queue and play.
    PlaylistTracksForQueue(Vec<Track>),
    /// An imported playlist file was chosen → parse + save as a new playlist.
    ImportPlaylistFile {
        /// Absolute path to the `.m3u`/`.pls` file.
        path: String,
        /// Suggested playlist name (file stem).
        name: String,
    },
    /// A folder was chosen for batch import → parse every playlist file in it.
    ImportPlaylistFolder(PathBuf),
    /// Import finished → `usize` tracks were matched and added.
    PlaylistImported(usize),
    /// A transport command from the smartphone remote (worker thread) — reuses
    /// the normal playback handler.
    PlaybackRemote(AudioPlaybackBarOutput),
    /// Open the sleep-timer dialog (main menu).
    ShowSleepTimer,
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
    /// Undo the last queue edit (Ctrl+Z).
    Undo,
    /// Redo the last undone queue edit (Ctrl+Shift+Z / Ctrl+Y).
    Redo,
    /// Toggle shuffle from a keyboard/action shortcut.
    ToggleShuffleShortcut,
    /// An MPRIS D-Bus command (`cmd`, optional µs/index arg) routed onto the
    /// main thread.
    MprisCommand(String, Option<i64>),
    /// Album art resolved for a track (async); `gen` guards against a stale
    /// result → MPRIS metadata + blur bg.
    ArtResolved {
        /// Track generation this fetch was started for.
        gen: u64,
        /// Raw art bytes, or `None`.
        data: Option<Vec<u8>>,
    },
    /// Album art cached to disk off the main thread (async); `gen` guards
    /// against a stale result → store `artUrl` + emit MPRIS metadata.
    MprisArtSaved {
        /// Track generation the cache write was started for.
        gen: u64,
        /// `file://` URL of the cached art, or `None` if the write failed.
        url: Option<String>,
    },
    /// Existing playlists resolved for the add-to-playlist picker (async DB
    /// result) → open the picker in the library panel.
    AddToPlaylistResolved {
        /// Track to add.
        track: Track,
        /// `(id, name)` of existing playlists.
        playlists: Vec<(i64, String)>,
    },
}

thread_local! {
    /// Main-thread handle to the running shell's input, set at init so the
    /// `AudioApplication` `open` handler can deliver opened files. The inputs
    /// carry GTK objects (not `Send`), so a thread-local `Sender` is used
    /// instead of a `MessageBroker` static.
    static SHELL_INPUT: std::cell::RefCell<Option<relm4::Sender<AudioShellInput>>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn set_shell_input(sender: relm4::Sender<AudioShellInput>) {
    SHELL_INPUT.with(|cell| {
        *cell.borrow_mut() = Some(sender);
    });
}

/// Deliver a message to the running shell from the main thread (used by the
/// application `open` handler). No-op if the shell is not running yet.
pub fn send_to_shell(message: AudioShellInput) {
    SHELL_INPUT.with(|cell| {
        if let Some(sender) = cell.borrow().as_ref() {
            let _ = sender.send(message);
        }
    });
}
