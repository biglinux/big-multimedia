//! Async channel-logo fetcher with on-disk cache (Relm4 port).
//!
//! Logos (`tvg-logo="..."`) point to arbitrary image URLs on the provider's
//! infrastructure. We:
//!
//! 1. Map the URL to a stable filename in `~/.cache/big-video-player/iptv-logos/`.
//! 2. If the file exists, return its path immediately (no network).
//! 3. Otherwise spawn a blocking download and write the result.
//!
//! The helper returns a GIO file; the UI calls `gtk::Image::set_from_file`
//! which handles decoding and scaling itself. Fetches that fail leave the cache
//! untouched and the UI falls back to an icon. Mirrors the legacy
//! `iptv_view::logos` until the V6 cutover deletes it.

use std::fs;
use std::path::PathBuf;

use gtk::glib;

use crate::config;
use big_app_kit::http_client::{RequestHeaders, http_get_bytes_with_headers};

/// Logo cache directory, created on demand.
#[must_use]
pub fn cache_dir() -> PathBuf {
    let dir = glib::user_cache_dir()
        .join(config::video::CONFIG_DIR)
        .join("iptv-logos");
    let _ = fs::create_dir_all(&dir);
    dir
}

// FNV-1a 64-bit; deterministic without pulling in a crypto dep.
#[must_use]
fn cache_file_for_url(url: &str) -> PathBuf {
    cache_file_for_url_in_dir(cache_dir(), url)
}

fn cache_file_for_url_in_dir(cache_dir: PathBuf, url: &str) -> PathBuf {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let ext = extension_from_url(url).unwrap_or("img");
    cache_dir.join(format!("{hash:016x}.{ext}"))
}

/// Pick a known image extension from the URL path, ignoring the query string.
fn extension_from_url(url: &str) -> Option<&'static str> {
    let clean = url.split('?').next().unwrap_or(url);
    let ext = std::path::Path::new(clean)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "webp" => Some("webp"),
        "gif" => Some("gif"),
        _ => None,
    }
}

/// Blocking, cache-aware logo byte fetch for the virtualized channel list.
///
/// Returns the logo bytes: the on-disk cache first (no network), else a headered
/// HTTP GET whose result is written back to the cache. Meant to run **off the
/// main thread** — `MediaListView` calls it on a worker for visible rows only,
/// then decodes + memory-caches the texture and cancels on recycle. No-op
/// (`None`) for an empty URL or on fetch failure.
#[must_use]
pub fn fetch_bytes(url: &str, user_agent: &str, referer: &str) -> Option<Vec<u8>> {
    if url.is_empty() {
        return None;
    }
    let path = cache_file_for_url(url);
    if let Ok(bytes) = fs::read(&path) {
        return Some(bytes);
    }
    let headers = RequestHeaders {
        user_agent,
        referer,
    };
    let bytes = http_get_bytes_with_headers(url, &headers)?;
    // Atomic: a cut-off write would otherwise be served from cache forever.
    if let Err(error) = big_os_kit::storage::atomic_write(&path, &bytes) {
        log::debug!("iptv logo cache write failed: {error}");
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_detected() {
        assert_eq!(extension_from_url("http://a/b.png"), Some("png"));
        assert_eq!(extension_from_url("http://a/b.jpg?size=64"), Some("jpg"));
        assert_eq!(extension_from_url("http://a/b"), None);
    }

    #[test]
    fn cache_filename_stable() {
        let cache_dir = PathBuf::from("/tmp/big-video-player-test-cache");
        let p1 = cache_file_for_url_in_dir(cache_dir.clone(), "http://example.com/logo.png");
        let p2 = cache_file_for_url_in_dir(cache_dir, "http://example.com/logo.png");
        assert_eq!(p1, p2);
    }

    #[test]
    fn cache_filename_differs_per_url() {
        let cache_dir = PathBuf::from("/tmp/big-video-player-test-cache");
        let p1 = cache_file_for_url_in_dir(cache_dir.clone(), "http://example.com/a.png");
        let p2 = cache_file_for_url_in_dir(cache_dir, "http://example.com/b.png");
        assert_ne!(p1, p2);
    }

    #[test]
    fn fetch_bytes_returns_none_for_empty() {
        assert!(fetch_bytes("", "ua", "ref").is_none());
    }
}
