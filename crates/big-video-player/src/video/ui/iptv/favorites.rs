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

/// Room for every channel of a large provider, far below the 1 MiB file limit.
const MAX_FAVORITES: usize = 4096;

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
        Self::load_from(Self::path())
    }
    fn load_from(path: PathBuf) -> Self {
        let (mut favorites, writer) = big_os_kit::deferred::Document::load(path, |bytes| {
            serde_json::from_slice::<Self>(bytes)
                .ok()
                .filter(|favorites| favorites.urls.len() <= MAX_FAVORITES)
        });
        favorites.persistence = Some(writer.merging(Self::merge_saved));
        favorites
    }
    /// The favorites a second window saved meanwhile, with this window's stars
    /// and unstars applied over them.
    fn merge_saved(base: Option<&[u8]>, ours: &[u8], theirs: Option<&[u8]>) -> Option<Vec<u8>> {
        let urls = |bytes: Option<&[u8]>| match bytes {
            None => Some(BTreeSet::new()),
            Some(bytes) => serde_json::from_slice::<Self>(bytes)
                .ok()
                .map(|saved| saved.urls),
        };
        let (base, ours, mut urls) = (urls(base)?, urls(Some(ours))?, urls(theirs)?);
        urls.extend(ours.difference(&base).cloned());
        urls.retain(|url| ours.contains(url) || !base.contains(url));
        if urls.len() > MAX_FAVORITES {
            return None;
        }
        serde_json::to_vec_pretty(&Self {
            urls,
            persistence: None,
        })
        .ok()
    }
    /// Admission is not disk success. See save_status for acknowledgement.
    pub fn save(&mut self) {
        let writer = self
            .persistence
            .get_or_insert_with(|| {
                big_os_kit::deferred::Document::reviewed(Self::path(), None)
                    .merging(Self::merge_saved)
            })
            .clone();
        if self.urls.len() > MAX_FAVORITES {
            writer.reject_update();
            log::warn!("too many IPTV favorites; not saved");
            return;
        }
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

    /// Star `to` in place of each starred `from`, and persist if any moved.
    pub fn rename(&mut self, renames: impl IntoIterator<Item = (String, String)>) {
        if self.rename_in_memory(renames) {
            self.save();
        }
    }

    fn rename_in_memory(&mut self, renames: impl IntoIterator<Item = (String, String)>) -> bool {
        let mut moved = false;
        for (from, to) in renames {
            if from != to && self.urls.remove(&from) {
                self.urls.insert(to);
                moved = true;
            }
        }
        moved
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
    fn rename_moves_only_starred_urls() {
        let mut f = IptvFavorites::default();
        f.toggle_in_memory("http://srv/live/u/p/1.ts");
        f.toggle_in_memory("http://other/2.ts");
        let renames = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(from, to)| ((*from).to_owned(), (*to).to_owned()))
                .collect::<Vec<_>>()
        };
        assert!(f.rename_in_memory(renames(&[
            (
                "http://srv/live/u/p/1.ts",
                "http://srv/live/{credentials}/1.ts"
            ),
            (
                "http://srv/live/u/p/3.ts",
                "http://srv/live/{credentials}/3.ts"
            ),
        ])));
        assert_eq!(
            f.urls.iter().map(String::as_str).collect::<Vec<_>>(),
            ["http://other/2.ts", "http://srv/live/{credentials}/1.ts"]
        );
        assert!(!f.rename_in_memory(renames(&[("http://other/2.ts", "http://other/2.ts")])));
    }

    #[test]
    fn oversized_favorites_are_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("iptv-favorites.json");
        let mut f = IptvFavorites::load_from(path.clone());
        for n in 0..=MAX_FAVORITES {
            f.toggle_in_memory(&format!("http://srv/{n}.ts"));
        }
        f.save();
        assert_eq!(
            f.save_status().unwrap().phase,
            big_os_kit::deferred::Phase::Failed
        );
        assert!(!path.exists());

        let oversized = serde_json::to_vec(&f).unwrap();
        std::fs::write(&path, &oversized).unwrap();
        let mut loaded = IptvFavorites::load_from(path.clone());
        assert!(!loaded.contains("http://srv/0.ts"));
        loaded.toggle("http://srv/0.ts");
        assert_eq!(std::fs::read(&path).unwrap(), oversized);
    }

    #[test]
    fn merge_keeps_both_windows_stars() {
        let saved = |urls: &[&str]| {
            let mut favorites = IptvFavorites::default();
            for url in urls {
                favorites.toggle_in_memory(url);
            }
            serde_json::to_vec(&favorites).unwrap()
        };
        let base = saved(&["http://a", "http://b"]);
        // This window starred c and unstarred a; the other one starred d.
        let ours = saved(&["http://b", "http://c"]);
        let theirs = saved(&["http://a", "http://b", "http://d"]);
        let merged = IptvFavorites::merge_saved(Some(&base), &ours, Some(&theirs)).unwrap();
        let merged: IptvFavorites = serde_json::from_slice(&merged).unwrap();
        assert_eq!(
            merged.urls.iter().map(String::as_str).collect::<Vec<_>>(),
            ["http://b", "http://c", "http://d"]
        );
        assert!(IptvFavorites::merge_saved(Some(&base), &ours, Some(b"{")).is_none());
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
