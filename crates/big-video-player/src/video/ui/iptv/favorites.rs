//! IPTV channel favorites — simple JSON set of channel URLs (Relm4 port).
//!
//! Keyed by URL (not by channel name): the same name can appear in many
//! providers with different stream endpoints, so URL is the only stable key
//! across re-fetches. Favorites are intentionally a flat set — no grouping or
//! ordering logic, just "starred or not".
//!
//! Engine-free helper (no GTK widgets). Mirrors the legacy
//! `iptv_view::favorites` until the V6 cutover deletes it.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use gtk::glib;
use serde::{Deserialize, Serialize};

use crate::config;
use crate::video::redaction::redact_path;

/// Flat set of starred channel stream URLs, persisted as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IptvFavorites {
    #[serde(default)]
    urls: BTreeSet<String>,
}

impl IptvFavorites {
    fn path() -> PathBuf {
        let base = glib::user_config_dir().join(config::video::CONFIG_DIR);
        let _ = fs::create_dir_all(&base);
        base.join("iptv-favorites.json")
    }

    #[must_use]
    pub fn load() -> Self {
        let path = Self::path();
        let Ok(content) = fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&content).unwrap_or_else(|err| {
            log::warn!("iptv favorites file unreadable ({err}); starting fresh");
            Self::default()
        })
    }

    pub fn save(&self) {
        let path = Self::path();
        match serde_json::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = fs::write(&path, s) {
                    // PII: redact user-home path.
                    log::warn!("failed to write {}: {e}", redact_path(&path));
                }
            }
            Err(e) => log::warn!("favorites serialize: {e}"),
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
