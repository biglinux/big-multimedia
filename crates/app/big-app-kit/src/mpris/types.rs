// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Typed values exchanged between the D-Bus service and the player.

/// Playback state as MPRIS2 spells it on the bus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaybackStatus {
    /// Media is advancing.
    Playing,
    /// Media is loaded but held at the current position.
    Paused,
    /// No media is active.
    #[default]
    Stopped,
}
impl PlaybackStatus {
    /// The exact string MPRIS2 expects for the `PlaybackStatus` property.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
        }
    }
}
/// Typed intentions. Track identity survives the bus -> GTK mailbox crossing.
#[derive(Debug, Clone, PartialEq)]
pub enum MprisCommand {
    /// Bring the player window to the front.
    Raise,
    /// Shut the application down.
    Quit,
    /// Skip to the next queue entry.
    Next,
    /// Skip to the previous queue entry.
    Previous,
    /// Resume playback.
    Play,
    /// Hold playback at the current position.
    Pause,
    /// Toggle between playing and paused.
    PlayPause,
    /// Stop playback and release the position.
    Stop,
    /// Move the position by a relative offset.
    Seek {
        /// Track the offset applies to; a stale id must be ignored.
        track_id: String,
        /// Signed offset in microseconds.
        offset_us: i64,
    },
    /// Move the position to an absolute point.
    SetPosition {
        /// Track the position applies to; a stale id must be ignored.
        track_id: String,
        /// Absolute position in microseconds.
        position_us: i64,
    },
    /// Load and play the given URI.
    OpenUri(String),
    /// Set the volume, expressed as the MPRIS 0.0–1.0 ratio.
    SetVolume(f64),
    /// Set the playback rate.
    SetRate(f64),
    /// Enable or disable shuffle.
    SetShuffle(bool),
    /// Set the loop mode (`None`, `Track` or `Playlist`).
    SetLoopStatus(String),
}
/// Sink the service calls on the GTK side for each decoded bus command.
pub type CommandCallback = Box<dyn Fn(MprisCommand) + Send + Sync + 'static>;
/// Source the service reads for the current player snapshot.
pub type StateCallback = Box<dyn Fn() -> PlayerState + Send + Sync + 'static>;
/// One coherent snapshot, published only after processing a player/queue event.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerState {
    /// Current playback state.
    pub status: PlaybackStatus,
    /// Identity of the active track, absent when nothing is loaded.
    pub track_id: Option<String>,
    /// Position within the track, in seconds.
    pub position_secs: f64,
    /// Track length in seconds; zero when unknown.
    pub duration_secs: f64,
    /// Volume as a 0–100 percentage, converted on publication.
    pub volume_pct: f64,
    /// Playback rate, where 1.0 is nominal speed.
    pub rate: f64,
    /// Track title.
    pub title: String,
    /// Track artist.
    pub artist: String,
    /// Track album.
    pub album: String,
    /// Cover art URI; empty when none is available.
    pub art_url: String,
    /// Whether a next entry can be selected.
    pub can_next: bool,
    /// Whether a previous entry can be selected.
    pub can_prev: bool,
    /// Whether playback can start.
    pub can_play: bool,
    /// Whether playback can be paused.
    pub can_pause: bool,
    /// Whether the position can be moved.
    pub can_seek: bool,
    /// Loop mode (`None`, `Track` or `Playlist`).
    pub loop_status: String,
    /// Whether shuffle is active.
    pub shuffle: bool,
}
pub(super) fn micros(value: f64) -> i64 {
    if value.is_finite() && value > 0.0 {
        (value * 1_000_000.0).min(i64::MAX as f64).round() as i64
    } else {
        0
    }
}
pub(super) fn volume(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0) / 100.0
    } else {
        0.0
    }
}
pub(super) fn rate(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value.clamp(0.25, 4.0)
    } else {
        1.0
    }
}
