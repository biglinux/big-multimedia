//! IPTV channel favorites: a JSON set of channel URLs.
//!
//! Keyed by URL (not by channel name): the same name can appear in many
//! providers with different stream endpoints, so URL is the only stable key
//! across re-fetches. Favorites are a flat set with no grouping or
//! ordering logic, just "starred or not".
//!
//! Engine-free helper (no GTK widgets).

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config;

/// Flat set of starred channel stream URLs, persisted as JSON.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct IptvFavorites {
    #[serde(default)]
    urls: BTreeSet<String>,
    #[serde(skip)]
    persistence: Option<big_os_kit::deferred::Document>,
}

impl std::fmt::Debug for IptvFavorites {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IptvFavorites")
            .field("entries", &self.urls.len())
            .finish_non_exhaustive()
    }
}
impl IptvFavorites {
    fn path() -> PathBuf {
        big_os_kit::xdg_dirs::config_dir()
            .map(|p| {
                p.join(config::video::CONFIG_DIR)
                    .join("iptv-favorites.json")
            })
            .unwrap_or_default()
    }
    #[must_use]
    pub fn load() -> Self {
        use big_os_kit::{deferred::Document, storage::read_regular_limited};
        let path = Self::path();
        let (mut favorites, writer) =
            match read_regular_limited(&path, big_os_kit::deferred::MAX_BYTES) {
                Ok(None) => (Self::default(), Document::reviewed(path, None)),
                Ok(Some(bytes)) => match serde_json::from_slice::<Self>(&bytes) {
                    Ok(value) => (value, Document::reviewed(path, Some(bytes))),
                    Err(_) => (Self::default(), Document::blocked(path)),
                },
                Err(_) => (Self::default(), Document::blocked(path)),
            };
        favorites.persistence = Some(writer);
        favorites
    }
    /// Admission is not disk success. See save_status for acknowledgement.
    pub fn save(&mut self) {
        let writer = self
            .persistence
            .get_or_insert_with(|| big_os_kit::deferred::Document::reviewed(Self::path(), None))
            .clone();
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            if writer.replace(bytes).is_err() {
                log::warn!("IPTV favorites are session-only until save is retried");
            }
        } else {
            writer.reject_update();
            log::warn!("IPTV favorites encoding failed");
        }
    }
    pub fn save_status(&self) -> Option<big_os_kit::deferred::Status> {
        self.persistence
            .as_ref()
            .map(big_os_kit::deferred::Document::status)
    }
    pub fn retry_save(&self) {
        if let Some(p) = &self.persistence {
            let _ = p.retry();
        }
    }

    #[must_use]
    pub fn contains(&self, url: &str) -> bool {
        self.urls.contains(url)
    }

    /// Flip the starred state of `url`, persist, and return the new state.
    pub fn toggle(&mut self, url: &str) -> bool {
        let now_starred = self.toggle_in_memory(url);
        self.save();
        now_starred
    }

    fn toggle_in_memory(&mut self, url: &str) -> bool {
        if self.urls.contains(url) {
            self.urls.remove(url);
            false
        } else {
            self.urls.insert(url.to_string());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_round_trips() {
        let mut f = IptvFavorites::default();
        assert!(!f.contains("http://x"));
        assert!(f.toggle_in_memory("http://x"));
        assert!(f.contains("http://x"));
        assert!(!f.toggle_in_memory("http://x"));
        assert!(!f.contains("http://x"));
    }

    #[test]
    fn roundtrip_json() {
        let mut f = IptvFavorites::default();
        f.toggle_in_memory("http://a");
        f.toggle_in_memory("http://b");
        let s = serde_json::to_string(&f).unwrap();
        let back: IptvFavorites = serde_json::from_str(&s).unwrap();
        assert!(back.contains("http://a"));
        assert!(back.contains("http://b"));
    }
}
