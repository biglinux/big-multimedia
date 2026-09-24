//! On-disk cache for IPTV playlists fetched over HTTP (Relm4 port).
//!
//! Fetching an M3U/Xtream list can be slow (5–30s on weak providers), so the
//! IPTV browser opens by showing the cached copy immediately and triggers a
//! background refresh. The refresh only replaces the cache if the new fetch
//! succeeds.
//!
//! Cache location: `$XDG_CACHE_HOME/big-video-player/iptv/<slug>.m3u`.
//!
//! Engine-free helper (no GTK widgets) used by [`super::VideoIptvBrowser`].
//! Mirrors the legacy `iptv_view::cache` until the V6 cutover deletes it.

use std::fs;
use std::path::PathBuf;

use big_app_kit::files::safe_filename_stem;
use gtk::glib;

use crate::config;
use crate::video::redaction::redact_path;

/// Compute the cache path for a given provider identifier.
#[must_use]
pub fn cache_path(provider_id: &str) -> PathBuf {
    let base = glib::user_cache_dir()
        .join(config::video::CONFIG_DIR)
        .join("iptv");
    let _ = fs::create_dir_all(&base);
    base.join(format!(
        "{}.m3u",
        safe_filename_stem(provider_id, 80, "unnamed")
    ))
}

/// Read the cached playlist content if it exists.
#[must_use]
pub fn read(provider_id: &str) -> Option<String> {
    fs::read_to_string(cache_path(provider_id)).ok()
}

/// Write the fetched content to the cache (best-effort).
pub fn write(provider_id: &str, content: &str) {
    let path = cache_path(provider_id);
    if let Err(e) = big_os_kit::storage::atomic_write(&path, content.as_bytes()) {
        // PII: redact user-home path.
        log::warn!("iptv cache write {}: {e}", redact_path(&path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_keeps_alphanumeric_and_safe_symbols() {
        assert_eq!(safe_filename_stem("My IPTV", 80, "unnamed"), "my_iptv");
        assert_eq!(
            safe_filename_stem("http://a.example/x.m3u", 80, "unnamed"),
            "http___a.example_x.m3u"
        );
    }

    #[test]
    fn slug_empty_becomes_unnamed() {
        assert_eq!(safe_filename_stem("", 80, "unnamed"), "unnamed");
    }

    #[test]
    fn slug_is_bounded() {
        let long = "a".repeat(200);
        assert_eq!(safe_filename_stem(&long, 80, "unnamed").len(), 80);
    }
}
