//! Radio favicon cache (memory + disk), explore-results cache, and
//! initials-texture generator for stations without icons.

#[cfg(test)]
use big_media_components::initials_avatar::{extract_initials, hsl_to_rgb};
// Only the radio panel's explore cache needs the radio data types. `media_list`
// (which the video player mounts) uses the texture cache above and nothing else,
// so gating here keeps `big-media-data/radio` out of the video build.
#[cfg(feature = "audio-panels")]
use big_media_data::radio::RadioExploreEntry;
use big_os_kit::http_client::{RequestHeaders, http_get_bytes_capped};
use gtk::gdk;
use log::warn;

const MAX_FAVICON_BYTES: u64 = 256 * 1024;

pub use big_media_components::initials_avatar::initials_texture as radio_initials_texture;
pub use big_media_components::initials_avatar::initials_widget as radio_initials_widget;

/// Per-app subdir (under the XDG cache dir) holding the radio favicon and
/// explore caches. Set once by the host app so caches don't collide between
/// apps; defaults to this crate's name when unset.
static CACHE_NAMESPACE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Set the on-disk cache namespace (the per-app cache subdir). Call once at
/// startup, before the radio panel runs. No-op if already set. Keeping this
/// caller-supplied is what keeps the cache generic — any media app provides its
/// own id instead of a hardcoded application name.
pub fn set_cache_namespace(name: impl Into<String>) {
    let _ = CACHE_NAMESPACE.set(name.into());
}

fn cache_namespace() -> &'static str {
    CACHE_NAMESPACE
        .get()
        .map_or(env!("CARGO_PKG_NAME"), String::as_str)
}

/// Bounded favicon-texture cache. `lru::LruCache` has no `Default`, so this
/// newtype provides one (cap 100, evicts the least-recently-used on overflow).
pub struct FaviconLruCache(pub lru::LruCache<String, gdk::Texture>);

impl Default for FaviconLruCache {
    fn default() -> Self {
        Self(lru::LruCache::new(
            std::num::NonZeroUsize::new(100).expect("100 is non-zero"),
        ))
    }
}

/// Returns `None` when the XDG cache dir is unavailable. Refusing to
/// fall back to `/tmp` closes the symlink-attack vector on a world-writable
/// directory (Stage 5 M2).
fn favicon_cache_dir() -> Option<std::path::PathBuf> {
    Some(
        big_os_kit::xdg_dirs::cache_dir()?
            .join(cache_namespace())
            .join("favicons"),
    )
}

fn favicon_cache_key(url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Disk-cached favicon fetch; silent on bad URLs since many Radio Browser
/// entries have broken/missing links.
pub fn fetch_favicon_bytes(url: &str) -> Option<Vec<u8>> {
    if url.is_empty() || url == "null" || !url.starts_with("http") {
        return None;
    }

    let Some(cache_dir) = favicon_cache_dir() else {
        log::debug!("[radio] favicon cache: no XDG cache dir; skipping fetch");
        return None;
    };
    let cache_file = cache_dir.join(favicon_cache_key(url));

    if let Ok(cached_favicon_bytes) = std::fs::read(&cache_file)
        && !cached_favicon_bytes.is_empty()
    {
        return Some(cached_favicon_bytes);
    }
    let favicon_response = match http_get_bytes_capped(
        url,
        &RequestHeaders::default(),
        MAX_FAVICON_BYTES,
        std::time::Duration::from_secs(15),
    ) {
        Ok(response) => response,
        Err(error) => {
            log::debug!("[radio] favicon fetch skip: {error}");
            return None;
        }
    };
    if favicon_response.status != 200 {
        log::debug!(
            "[radio] favicon HTTP {}: {}",
            favicon_response.status,
            big_os_kit::url::MediaLogTarget(url)
        );
        return None;
    }
    let favicon_bytes = favicon_response.bytes;

    if let Err(error) = std::fs::create_dir_all(&cache_dir) {
        warn!("[radio] favicon cache dir fail: {error}");
    }
    if let Err(error) = std::fs::write(&cache_file, &favicon_bytes) {
        warn!("[radio] favicon cache write fail: {error}");
    }

    Some(favicon_bytes)
}

fn explore_cache_path() -> Option<std::path::PathBuf> {
    Some(
        big_os_kit::xdg_dirs::cache_dir()?
            .join(cache_namespace())
            .join("radio_explore.json"),
    )
}

/// Persist the popular/explore station list to the XDG cache as JSON.
/// No-ops (with a warning) if no cache dir is available.
#[cfg(feature = "audio-panels")]
pub fn save_explore_cache(results: &[RadioExploreEntry]) {
    let Some(path) = explore_cache_path() else {
        warn!("[radio] explore cache: no XDG cache dir; skipping save");
        return;
    };
    let json: Vec<serde_json::Value> = results
        .iter()
        .map(|(name, url, country, tags, favicon_url)| {
            serde_json::json!([name, url, country, tags, favicon_url])
        })
        .collect();
    if let Ok(explore_cache_json) = serde_json::to_string(&json) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(&path, explore_cache_json) {
            warn!("[radio] explore cache write fail: {error}");
        }
    } else {
        warn!("[radio] explore cache serialize fail");
    }
}

/// Load the cached popular/explore station list, or an empty vec when the
/// cache is missing or unreadable.
#[cfg(feature = "audio-panels")]
pub fn load_explore_cache() -> Vec<RadioExploreEntry> {
    let Some(path) = explore_cache_path() else {
        return Vec::new();
    };
    let Ok(explore_cache_json) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let cached_station_values: Vec<serde_json::Value> =
        match serde_json::from_str(&explore_cache_json) {
            Ok(station_values) => station_values,
            Err(error) => {
                warn!("[radio] explore cache JSON parse fail: {error}");
                return Vec::new();
            }
        };
    cached_station_values
        .iter()
        .filter_map(|entry_value| {
            let entry_fields = entry_value.as_array()?;
            if entry_fields.len() >= 5 {
                Some((
                    entry_fields[0].as_str().unwrap_or("").to_string(),
                    entry_fields[1].as_str().unwrap_or("").to_string(),
                    entry_fields[2].as_str().unwrap_or("").to_string(),
                    entry_fields[3].as_str().unwrap_or("").to_string(),
                    entry_fields[4].as_str().unwrap_or("").to_string(),
                ))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_two_words() {
        assert_eq!(extract_initials("Big Radio"), "BR");
    }

    #[test]
    fn initials_single_word() {
        assert_eq!(extract_initials("Jazz"), "J");
    }

    #[test]
    fn initials_empty_name() {
        assert_eq!(extract_initials(""), "R");
    }

    #[test]
    fn initials_three_words_takes_first_two() {
        assert_eq!(extract_initials("Classic Rock FM"), "CR");
    }

    #[test]
    fn hsl_red() {
        let (r, g, b) = hsl_to_rgb(0.0, 1.0, 0.5);
        assert!((r - 1.0).abs() < 0.01);
        assert!(g.abs() < 0.01);
        assert!(b.abs() < 0.01);
    }

    #[test]
    fn hsl_grey() {
        let (r, g, b) = hsl_to_rgb(0.0, 0.0, 0.5);
        assert!((r - 0.5).abs() < 0.01);
        assert!((g - 0.5).abs() < 0.01);
        assert!((b - 0.5).abs() < 0.01);
    }

    #[test]
    fn cache_key_is_hex() {
        let key = favicon_cache_key("https://example.com/icon.png");
        assert_eq!(key.len(), 16);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn cache_key_deterministic() {
        let k1 = favicon_cache_key("https://radio.example.com");
        let k2 = favicon_cache_key("https://radio.example.com");
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_different_urls_differ() {
        let k1 = favicon_cache_key("https://a.example.com");
        let k2 = favicon_cache_key("https://b.example.com");
        assert_ne!(k1, k2);
    }

    #[cfg(feature = "audio-panels")]
    #[test]
    fn explore_cache_roundtrip() {
        let entries: Vec<RadioExploreEntry> = vec![
            (
                "Station A".into(),
                "https://a.com".into(),
                "US".into(),
                "pop,rock".into(),
                "https://favicon.a.com/icon.png".into(),
            ),
            (
                "Station B".into(),
                "https://b.com".into(),
                "BR".into(),
                "jazz".into(),
                String::new(),
            ),
        ];

        let json: Vec<serde_json::Value> = entries
            .iter()
            .map(|(name, url, country, tags, favicon_url)| {
                serde_json::json!([name, url, country, tags, favicon_url])
            })
            .collect();
        let explore_cache_json = serde_json::to_string(&json).unwrap();
        let cached_station_values: Vec<serde_json::Value> =
            serde_json::from_str(&explore_cache_json).unwrap();
        let restored: Vec<RadioExploreEntry> = cached_station_values
            .iter()
            .filter_map(|entry_value| {
                let entry_fields = entry_value.as_array()?;
                if entry_fields.len() >= 5 {
                    Some((
                        entry_fields[0].as_str().unwrap_or("").to_string(),
                        entry_fields[1].as_str().unwrap_or("").to_string(),
                        entry_fields[2].as_str().unwrap_or("").to_string(),
                        entry_fields[3].as_str().unwrap_or("").to_string(),
                        entry_fields[4].as_str().unwrap_or("").to_string(),
                    ))
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].0, "Station A");
        assert_eq!(restored[1].3, "jazz");
        assert_eq!(restored[1].4, "");
    }

    #[test]
    fn fetch_favicon_rejects_null_url() {
        assert!(fetch_favicon_bytes("null").is_none());
    }

    #[test]
    fn fetch_favicon_rejects_empty_url() {
        assert!(fetch_favicon_bytes("").is_none());
    }

    #[test]
    fn fetch_favicon_rejects_non_http() {
        assert!(fetch_favicon_bytes("ftp://example.com/icon.png").is_none());
        assert!(fetch_favicon_bytes("data:image/png;base64,abc").is_none());
    }

    #[test]
    fn initials_unicode_chars() {
        assert_eq!(extract_initials("Rádio Brasil"), "RB");
    }

    #[test]
    fn initials_numbers_prefix() {
        assert_eq!(extract_initials("101.5 FM"), "1F");
    }

    #[test]
    fn initials_whitespace_only() {
        assert_eq!(extract_initials("   "), " ");
    }

    #[test]
    fn initials_very_long_name() {
        assert_eq!(extract_initials("A Very Long Station Name Indeed"), "AV");
    }

    #[test]
    fn hsl_blue() {
        let (r, g, b) = hsl_to_rgb(240.0, 1.0, 0.5);
        assert!(r.abs() < 0.01);
        assert!(g.abs() < 0.01);
        assert!((b - 1.0).abs() < 0.01);
    }

    #[test]
    fn hsl_green() {
        let (r, g, b) = hsl_to_rgb(120.0, 1.0, 0.5);
        assert!(r.abs() < 0.01);
        assert!((g - 1.0).abs() < 0.01);
        assert!(b.abs() < 0.01);
    }

    #[test]
    fn hsl_black() {
        let (r, g, b) = hsl_to_rgb(0.0, 0.0, 0.0);
        assert!(r.abs() < 0.01);
        assert!(g.abs() < 0.01);
        assert!(b.abs() < 0.01);
    }

    #[test]
    fn hsl_white() {
        let (r, g, b) = hsl_to_rgb(0.0, 0.0, 1.0);
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 1.0).abs() < 0.01);
        assert!((b - 1.0).abs() < 0.01);
    }

    #[test]
    fn cache_key_url_with_query_params() {
        let k1 = favicon_cache_key("https://example.com/icon.png?v=1");
        let k2 = favicon_cache_key("https://example.com/icon.png?v=2");
        assert_ne!(k1, k2);
        assert_eq!(k1.len(), 16);
    }

    #[cfg(feature = "audio-panels")]
    #[test]
    fn explore_cache_empty_roundtrip() {
        let entries: Vec<RadioExploreEntry> = vec![];
        let json: Vec<serde_json::Value> = entries
            .iter()
            .map(|(name, url, country, tags, favicon_url)| {
                serde_json::json!([name, url, country, tags, favicon_url])
            })
            .collect();
        let explore_cache_json = serde_json::to_string(&json).unwrap();
        let cached_station_values: Vec<serde_json::Value> =
            serde_json::from_str(&explore_cache_json).unwrap();
        assert!(cached_station_values.is_empty());
    }
}
