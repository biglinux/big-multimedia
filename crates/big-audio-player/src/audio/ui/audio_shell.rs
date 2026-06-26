//! Relm4 root component for the audio player.
//!
//! `AudioShell` owns the engine-side state, database handles, queue, and child
//! panel controllers. Child panels emit typed outputs; the shell resolves them
//! into queue, engine, persistence, and dialog work, then pushes state back into
//! panels as typed inputs.

mod chrome;
mod component;
mod engine;
mod export;
mod input;
mod library_store;
mod metadata;
mod mpris;
mod panel_intents;
mod playback;
mod playlist_manager_actions;
mod queue_state;
mod sources;
mod update;

pub use input::{send_to_shell, AudioShellInput};

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use relm4::{Component, ComponentController, ComponentParts, ComponentSender, Controller};

use crate::audio::converter_launcher;
use crate::audio::library_grouping::{AgeFilter, LibraryGrouping};
use crate::audio::library_query;
use crate::audio::library_scan_engine;
use crate::audio::lyrics_fetch_engine;
use crate::audio::mpris_state::{save_mpris_art, SharedMprisState};
use crate::audio::playlist_import;
use crate::audio::queue_undo::QueueHistory;
use crate::audio::ui::library_panel::{LibraryPanel, LibraryPanelInput, LibraryPanelOutput};
use crate::audio::ui::library_tag_editor::TagEditFields;
use crate::audio::ui::library_tree::LibraryTreeData;
use crate::audio::ui::playlist_manager::{
    PlaylistManager, PlaylistManagerInput, PlaylistManagerOutput,
};
use crate::audio::ui::playlist_panel::{PlaylistPanel, PlaylistPanelInput, PlaylistPanelOutput};
use crate::db::models::{RadioStation, Track, WatchFolder};
use crate::db::queries_advanced::{update_track_metadata, TrackMetadataUpdate};
use crate::db::{self, queries, DbHandle};
use crate::i18n::i18n;
use crate::mpris::{CommandCallback, Mpris, PlayerState, StateCallback};
use crate::player::mpv_backend::{MpvBackend, PlayerEvent};
use crate::player::queue::{PlayQueue, RepeatMode};
use crate::settings::{keys, Settings};
use crate::shared::ui_helpers::spawn_db_query;
use big_app_kit::file_dialogs::FilePicker;
use big_app_kit::mpris::{
    route_common_playback_command, BigMprisPlaybackControls, BigMprisPlaybackFeedback,
};
use big_media_components::media::file_info_dialog::{
    show_media_file_info_dialog, BigMediaFileInfoDialogLabels, BigMediaFileInfoDialogSpec,
};
use big_media_components::playback::audio_transport::{
    BigAudioTransportLabels, BigAudioTransportSpec,
};
use big_media_components::player_chrome::spawn_blocking_result;
use big_media_components::time;
use big_player_ui::audio_trim_bar::{
    AudioTrimBar, AudioTrimBarInput, AudioTrimBarOutput, BigAudioTrimBarLabels,
};
use big_player_ui::download::filename_from_url;
use big_player_ui::download_panel::{DownloadManagerInput, DownloadManagerPanel};
use big_player_ui::file_browser::{FileBrowser, FileBrowserConfig, FileBrowserOutput};
use big_player_ui::lyrics_panel::{LyricsPanel, LyricsPanelInput, LyricsPanelOutput, LyricsState};
use big_player_ui::playback_bar::{
    AudioPlaybackBar, AudioPlaybackBarInit, AudioPlaybackBarInput, AudioPlaybackBarOutput,
};
use big_player_ui::podcast_panel::{PodcastPanel, PodcastPanelOutput};
use big_player_ui::radio_panel::{RadioPanel, RadioPanelInput, RadioPanelOutput};

/// Relm4 root component: the audio window + its child component tree + the
/// shell-owned play queue.
pub struct AudioShell {
    library: Controller<LibraryPanel>,
    /// Playlist manager dialog (save/load/rename/export/import/delete).
    playlist_manager: Controller<PlaylistManager>,
    playlist: Controller<PlaylistPanel>,
    /// Bottom transport bar (play/seek/volume/repeat/shuffle).
    playback_bar: Controller<AudioPlaybackBar>,
    /// Visual trim controls shown above the bottom transport in trim mode.
    trim: Controller<AudioTrimBar>,
    /// Files browser tab — held for its lifetime (Output forwarding); the shell
    /// sends it no inputs, so it is never read after construction.
    _files: Controller<FileBrowser>,
    /// Radio tab.
    radio: Controller<RadioPanel>,
    /// Lyrics panel — shares the Queue tab's vertical space with the playlist.
    lyrics: Controller<LyricsPanel>,
    /// Vertical split holding the lyrics panel (start) over the queue (end);
    /// the shell drives its position on show/expand.
    lyrics_paned: gtk::Paned,
    /// Whether the lyrics panel is shown (gates the on-demand fetch).
    lyrics_visible: bool,
    /// Slide-up download-manager panel (podcast episode downloads); self-driven
    /// once fed an `Enqueue`.
    downloads: Controller<DownloadManagerPanel>,
    /// Podcast tab (subscriptions + search); self-renders + emits `LoadFeed`,
    /// so the shell holds it only for its lifetime (never sends it inputs).
    _podcast: Controller<PodcastPanel>,
    /// Toast overlay for panel `Notify` messages.
    toast_overlay: adw::ToastOverlay,
    /// Whether the queue currently holds podcast episodes (suppresses the music
    /// queue + toggles the stash/restore on tab switches).
    podcast_mode: bool,
    /// Music queue stashed on entering podcast mode (tracks + current index).
    saved_music_queue: Option<(Vec<Track>, Option<usize>)>,
    /// Podcast queue stashed on leaving the podcast tab (restored on return).
    saved_podcast_queue: Option<(Vec<Track>, Option<usize>)>,
    queue: PlayQueue,
    /// Snapshot-based undo/redo history for user queue edits.
    history: QueueHistory,
    /// Bumped on every track load; async art/lyrics results carry the
    /// generation they were fetched for and are dropped if it has advanced
    /// (the user skipped to another track before the fetch returned).
    now_playing_gen: u64,
    /// Sidebar-bottom now-playing card (hidden until playback): box + art +
    /// title/artist/album labels.
    np_box: gtk::Box,
    np_art: gtk::Image,
    np_title: gtk::Label,
    np_artist: gtk::Label,
    np_album: gtk::Label,
    /// Whether voice-enhancement (noise reduction) is active (NR pipeline toggle
    /// crosses on this).
    voice_active: std::cell::Cell<bool>,
    /// Name of the currently-playing radio station (for recording filenames).
    radio_station_name: String,
    /// Destination of the in-progress radio recording (None = not recording).
    recording_path: Option<PathBuf>,
    /// MPRIS2 D-Bus service (None if the session bus is unavailable).
    mpris: Option<Mpris>,
    /// Playback-state snapshot read by the MPRIS `StateCallback` (D-Bus thread).
    mpris_state: Arc<SharedMprisState>,
    /// Duration of the loaded track (seek-percent → seconds conversion).
    duration_secs: f64,
    /// Whether visual trim mode is active.
    trim_active: bool,
    /// Sorted display ranges currently selected in trim mode.
    trim_ranges: Vec<(f64, f64)>,
    /// Whether playback should skip/loop through only selected cuts.
    trim_play_selection: bool,
    /// Audio playback backend (system libmpv). Built once at init; `None` only
    /// if the engine failed to initialize.
    mpv: Option<Arc<MpvBackend>>,
    /// Writer DB handle (queue persistence, ratings, playlists, deletes).
    db: Option<DbHandle>,
    /// Read-only DB handle (library/detail queries; avoids writer contention).
    db_read: Option<DbHandle>,
    /// Persisted settings (audio config, column widths).
    settings: Settings,
    /// Chromecast discovery/casting engine (shared runtime engine).
    #[cfg(feature = "chromecast")]
    cast_manager: Arc<crate::cast::chromecast::CastManager>,
    /// Active sleep-timer countdown source (None = no timer armed).
    sleep_timer_source: Option<glib::SourceId>,
    /// Seconds left on the armed sleep timer.
    sleep_timer_remaining: u32,
    /// Action to perform when the sleep timer expires.
    sleep_timer_action: String,
    /// Pre-timer volume, restored after the fade-to-pause on expiry.
    sleep_timer_volume: f64,
    /// Current library grouping preset (drives tree + node resolution).
    library_grouping: LibraryGrouping,
    /// Current library search filter text.
    library_filter: String,
    /// Current age-filter dropdown index.
    library_age: u32,
    /// Path of the loaded track (empty = nothing loaded).
    current_file: String,
    /// Last resolved cover bytes — kept so the blurred background can be toggled
    /// on/off (Appearance setting) without re-resolving the art.
    last_art_bytes: Option<Vec<u8>>,
    /// Whether playback is active (drives play-if-idle on enqueue).
    is_playing: bool,
    /// Whether the current playback is a radio stream (suppresses queue
    /// auto-advance on end-of-file).
    is_stream: bool,
    /// Active saved-playlist name (empty = ad-hoc queue); shown in the queue
    /// toolbar header. Set when a saved playlist is loaded (later commit).
    current_playlist_name: String,
}
