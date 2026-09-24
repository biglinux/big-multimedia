//! Shared MPRIS playback-state snapshot for the Relm4 `AudioShell`.
//!
//! The MPRIS `StateCallback` runs on the D-Bus thread, so the state it reads
//! must be `Send + Sync`. The shell mutates these fields from mpv events (on the
//! main thread) and the D-Bus thread reads them through an `Arc`. Ported out of
//! the god-object `window::SharedMprisState` (survives the cutover).

use big_app_kit::mpris::{PlaybackStatus, PlayerState};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
            status: Mutex::new(PlaybackStatus::Stopped),
            track_id: Mutex::new(None),
            sequence: AtomicU64::new(1),
            rate: Mutex::new(1.0),
            seekable: AtomicBool::new(false),
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
        *self
            .track_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(format!("/br/com/biglinux/AudioPlayer/Track/{id}"));
    }
    pub(crate) fn invalidate(&self) {
        self.track_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
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
    if data.is_empty() || data.len() > LIMIT {
        return None;
    }
    let cache_dir = dirs::cache_dir()?
        .join("big-audio-player")
        .join("mpris-art-cache");
    std::fs::create_dir_all(&cache_dir).ok()?;
    // Cooperative workers/processes share one short nonblocking cache lock.
    // This directory contains cache entries only, never original artwork.
    let _lock = big_app_kit::storage::WriteLock::try_acquire(&cache_dir.join("index")).ok()?;
    let mut entries = Vec::new();
    // Bound the scan, but well above the 32 entries eviction keeps: the lock
    // file and leftovers from interrupted writes also live here, and they must
    // not disable artwork once they pile up. Foreign names are skipped.
    for entry in std::fs::read_dir(&cache_dir).ok()?.take(1024) {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with("mpris-art-")
            && std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jpg"))
            && entry.file_type().ok()?.is_file()
        {
            let metadata = entry.metadata().ok()?;
            entries.push((metadata.modified().ok()?, metadata.len(), entry.path()));
        }
    }
    entries.sort_by_key(|entry| entry.0);
    let mut bytes = entries
        .iter()
        .try_fold(0_u64, |sum, entry| sum.checked_add(entry.1))?;
    let mut count = entries.len();
    for (_, size, path) in &entries {
        if count < 32 && bytes.saturating_add(data.len() as u64) <= 64 * 1024 * 1024 {
            break;
        }
        std::fs::remove_file(path).ok()?;
        count -= 1;
        bytes = bytes.saturating_sub(*size);
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
    let art_path = cache_dir.join(format!(
        "mpris-art-{}-{}-{nonce}.jpg",
        std::process::id(),
        gtk::glib::real_time()
    ));
    let stage = big_app_kit::storage::StagedFile::new(&art_path).ok()?;
    std::fs::write(stage.path(), data).ok()?;
    // Both receipts mean publication happened; cache durability uncertainty is
    // not a reason to overwrite the published entry with an older generation.
    let _receipt = stage.publish(&art_path).ok()?;
    Some(gtk::gio::File::for_path(&art_path).uri().to_string())
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn load_occurrences_do_not_share_track_id() {
        let state = SharedMprisState::default();
        state.selected();
        let first = state.track_id.lock().unwrap().clone();
        state.selected();
        assert_ne!(*state.track_id.lock().unwrap(), first);
        state.invalidate();
        assert!(state.track_id.lock().unwrap().is_none());
    }
}
