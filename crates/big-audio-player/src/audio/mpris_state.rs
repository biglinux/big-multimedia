//! Shared MPRIS playback-state snapshot for the Relm4 `AudioShell`.
//!
//! The MPRIS `StateCallback` runs on the D-Bus thread, so the state it reads
//! must be `Send + Sync`. The shell mutates these fields from mpv events (on the
//! main thread) and the D-Bus thread reads them through an `Arc`. Ported out of
//! the god-object `window::SharedMprisState` (survives the cutover).

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

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

#[cfg(all(test, not(miri)))]
mod tests {
    use super::save_mpris_art;
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Mutex, PoisonError};
    use std::time::{SystemTime, UNIX_EPOCH};

    static CACHE_ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

    struct CacheHomeOverride {
        previous_cache_home: Option<OsString>,
        cache_home: PathBuf,
    }

    impl CacheHomeOverride {
        fn activate() -> std::result::Result<Self, Box<dyn std::error::Error>> {
            let unique_suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let cache_home = env::temp_dir().join(format!(
                "big-audio-mpris-cache-{}-{unique_suffix}",
                std::process::id()
            ));
            fs::create_dir_all(&cache_home)?;
            let previous_cache_home = env::var_os("XDG_CACHE_HOME");
            // FIXME: Audit that the environment access only happens in single-threaded code.
            unsafe { env::set_var("XDG_CACHE_HOME", &cache_home) };
            Ok(Self {
                previous_cache_home,
                cache_home,
            })
        }
    }

    impl Drop for CacheHomeOverride {
        fn drop(&mut self) {
            if let Some(previous_cache_home) = &self.previous_cache_home {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                unsafe { env::set_var("XDG_CACHE_HOME", previous_cache_home) };
            } else {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                unsafe { env::remove_var("XDG_CACHE_HOME") };
            }
            let _ = fs::remove_dir_all(&self.cache_home);
        }
    }

    #[test]
    fn save_mpris_art_writes_bytes_to_app_cache_and_returns_file_uri()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let _environment_guard = CACHE_ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let cache_home_override = CacheHomeOverride::activate()?;
        let art_bytes = b"jpeg bytes for mpris";

        let art_url = save_mpris_art(art_bytes)
            .ok_or_else(|| std::io::Error::other("cache dir should be available"))?;
        let art_path = PathBuf::from(
            art_url
                .strip_prefix("file://")
                .ok_or_else(|| std::io::Error::other("MPRIS art URL should use file scheme"))?,
        );

        assert_eq!(
            art_path,
            cache_home_override
                .cache_home
                .join("big-audio-player")
                .join("mpris-art.jpg")
        );
        assert_eq!(
            fs::read(&art_path)?,
            art_bytes,
            "MPRIS art cache should contain the bytes supplied by the caller"
        );

        Ok(())
    }
}
