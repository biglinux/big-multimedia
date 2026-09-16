//! Shared MPRIS playback-state snapshot for the Relm4 `AudioShell`.
//!
//! The MPRIS `StateCallback` runs on the D-Bus thread, so the state it reads
//! must be `Send + Sync`. The shell mutates these fields from mpv events (on the
//! main thread) and the D-Bus thread reads them through an `Arc`. Ported out of
//! the god-object `window::SharedMprisState` (survives the cutover).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::mpris::{PlaybackStatus, PlayerState};

/// `Sync`-safe snapshot of the current playback state for MPRIS.
pub(crate) struct SharedMprisState {
    pub(crate) snapshot: Mutex<PlayerState>,
    pub(crate) status: Mutex<PlaybackStatus>,
    pub(crate) track_id: Mutex<Option<String>>,
    sequence: AtomicU64,
    pub(crate) rate: Mutex<f64>,
    pub(crate) seekable: AtomicBool,
    /// Whether playback is paused.
    pub(crate) paused: AtomicBool,
    /// Current position (seconds).
    pub(crate) position: Mutex<f64>,
    /// Track duration (seconds).
    pub(crate) duration: Mutex<f64>,
    /// Volume percent.
    pub(crate) volume: Mutex<f64>,
    /// Current track title.
    pub(crate) title: Mutex<String>,
    /// Current track artist.
    pub(crate) artist: Mutex<String>,
    /// Current track album.
    pub(crate) album: Mutex<String>,
    /// `"None"` | `"Track"` | `"Playlist"`.
    pub(crate) loop_status: Mutex<String>,
    /// Whether shuffle is on.
    pub(crate) shuffle: AtomicBool,
    /// `file://` URL of cached art (this player uses local cache URIs).
    pub(crate) art_url: Mutex<String>,
}

impl Default for SharedMprisState {
    fn default() -> Self {
        Self {
            snapshot: Mutex::new(PlayerState::default()),
            status: Mutex::new(PlaybackStatus::Stopped), track_id: Mutex::new(None),
            sequence: AtomicU64::new(1), rate: Mutex::new(1.0), seekable: AtomicBool::new(false),
            paused: AtomicBool::new(true),
            position: Mutex::new(0.0),
            duration: Mutex::new(0.0),
            volume: Mutex::new(100.0),
            title: Mutex::new(String::new()),
            artist: Mutex::new(String::new()),
            album: Mutex::new(String::new()),
            loop_status: Mutex::new("None".into()),
            shuffle: AtomicBool::new(false),
            art_url: Mutex::new(String::new()),
        }
    }
}

impl SharedMprisState {
    pub(crate) fn selected(&self) {
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        *self.track_id.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(format!("/br/com/biglinux/AudioPlayer/Track/{id}"));
    }
    pub(crate) fn invalidate(&self) {
        self.track_id.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        self.seekable.store(false, Ordering::Relaxed);
    }
}

/// This player caches `artUrl` as a local `file://` URI — write the art bytes to
/// the app cache and return the URL. Refuses to write when `dirs::cache_dir()`
/// is unavailable rather than falling back to world-writable `/tmp`.
///
/// Blocking (`create_dir_all` + `write`): run off the GTK main thread (see
/// `AudioShell::apply_resolved_art`, which offloads it via `spawn_blocking_result`).
pub(crate) fn save_mpris_art(data: &[u8]) -> Option<String> {
    use gtk::gio::prelude::FileExt;
    const LIMIT: usize = 16 * 1024 * 1024;
    if data.is_empty() || data.len() > LIMIT { return None; }
    let cache_dir = dirs::cache_dir()?.join("big-audio-player");
    std::fs::create_dir_all(&cache_dir).ok()?;
    // Stable content identity prevents an older generation's write from
    // changing the image behind an already-published URI. This is cache naming,
    // not an authentication or integrity hash.
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new(); data.hash(&mut hash);
    let art_path = cache_dir.join(format!("mpris-art-{:016x}.jpg", hash.finish()));
    std::fs::write(&art_path, data).ok()?;
    Some(gtk::gio::File::for_path(&art_path).uri().to_string())
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn load_occurrences_do_not_share_track_id() {
        let state = SharedMprisState::default(); state.selected();
        let first = state.track_id.lock().unwrap().clone();
        state.selected(); assert_ne!(*state.track_id.lock().unwrap(), first);
        state.invalidate(); assert!(state.track_id.lock().unwrap().is_none());
    }
}
