//! Shared MPRIS playback-state snapshot for the Relm4 `AudioShell`.
//!
//! The MPRIS `StateCallback` runs on the D-Bus thread, so the state it reads
//! must be `Send + Sync`. The shell mutates these fields from mpv events (on the
//! main thread) and the D-Bus thread reads them through an `Arc`. Ported out of
//! the god-object `window::SharedMprisState` (survives the cutover).

use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

/// `Sync`-safe snapshot of the current playback state for MPRIS.
pub(crate) struct SharedMprisState {
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
    /// `file://` URL of cached art (MPRIS requires file:// for `mpris:artUrl`).
    pub(crate) art_url: Mutex<String>,
}

impl Default for SharedMprisState {
    fn default() -> Self {
        Self {
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

/// MPRIS spec requires `artUrl` to be a `file://` URL — write the art bytes to
/// the app cache and return the URL. Refuses to write when `dirs::cache_dir()`
/// is unavailable rather than falling back to world-writable `/tmp`.
///
/// Blocking (`create_dir_all` + `write`): run off the GTK main thread (see
/// `AudioShell::apply_resolved_art`, which offloads it via `spawn_blocking_result`).
pub(crate) fn save_mpris_art(data: &[u8]) -> Option<String> {
    let cache_dir = dirs::cache_dir()?.join("big-audio-player");
    std::fs::create_dir_all(&cache_dir).ok()?;
    let art_path = cache_dir.join("mpris-art.jpg");
    std::fs::write(&art_path, data).ok()?;
    Some(format!("file://{}", art_path.display()))
}
