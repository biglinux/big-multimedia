//! IPTV provider model and persistent store.
//!
//! Users configure one or more IPTV providers through the management
//! dialog; each provider is a (name, source) pair plus optional HTTP
//! headers (`User-Agent`, `Referer`) that many IPTV servers require.
//!
//! Persistence format is TOML at
//! `$XDG_CONFIG_HOME/big-video-player/iptv-providers.toml` (default
//! `~/.config/big-video-player/iptv-providers.toml`). The file is
//! human-editable: users should be able to copy it between machines.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Source kinds understood by the IPTV subsystem.
///
/// `M3uUrl` / `M3uLocal` cover the vast majority of real-world usage;
/// `Xtream` adds support for the Xtream Codes API used by commercial
/// panels (category listing, VOD, series).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IptvKind {
    /// Remote M3U/M3U8 URL fetched over HTTP(S).
    M3uUrl { url: String },
    /// Local path to an M3U/M3U8 file on disk.
    M3uLocal { path: String },
    /// Xtream Codes server (`server`, `username`, `password`).
    Xtream {
        server: String,
        username: String,
        password: String,
    },
}

impl IptvKind {
    /// Human-readable single-line summary for the management dialog.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            IptvKind::M3uUrl { url } => url.clone(),
            IptvKind::M3uLocal { path } => path.clone(),
            IptvKind::Xtream {
                server, username, ..
            } => format!("{server} ({username})"),
        }
    }
}

/// One configured IPTV source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IptvProvider {
    /// User-facing name (`"My IPTV"`, `"Free List"`, …).
    pub name: String,
    /// How the channels get fetched.
    pub source: IptvKind,
    /// Optional `User-Agent` override sent with every request for this
    /// provider. Many real IPTV servers refuse the default Rust UA.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_agent: String,
    /// Optional `Referer` header.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub referer: String,
    /// User-set ordering for the management dialog. Smaller = earlier.
    /// Deserialises to 0 if absent; reassigned on every save.
    #[serde(default)]
    pub order: u32,
}

impl IptvProvider {
    #[must_use]
    pub fn new_m3u_url(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: IptvKind::M3uUrl { url: url.into() },
            user_agent: String::new(),
            referer: String::new(),
            order: 0,
        }
    }
}

impl Default for IptvProvider {
    fn default() -> Self {
        Self {
            name: String::new(),
            source: IptvKind::M3uUrl { url: String::new() },
            user_agent: String::new(),
            referer: String::new(),
            order: 0,
        }
    }
}

/// On-disk container; `#[serde(default)]` keeps `providers = []` when file
/// is missing or corrupt so we never surface parse errors to the user.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IptvProviderStore {
    #[serde(default)]
    pub providers: Vec<IptvProvider>,
}

impl IptvProviderStore {
    /// Return the canonical config-file path, creating the parent dir if
    /// it does not exist yet.
    pub fn config_path() -> PathBuf {
        let base = glib::user_config_dir().join(crate::config::video::CONFIG_DIR);
        let _ = fs::create_dir_all(&base);
        base.join("iptv-providers.toml")
    }

    /// Read the store from disk; missing/corrupt file yields an empty store.
    #[must_use]
    pub fn load() -> Self {
        let path = Self::config_path();
        let Ok(content) = fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&content).unwrap_or_else(|err| {
            log::warn!("iptv providers file unreadable ({err}); starting fresh");
            Self::default()
        })
    }

    /// Atomically write the store to disk. Errors are logged but not
    /// propagated — the in-memory state is authoritative for the session.
    pub fn save(&mut self) {
        for (i, p) in self.providers.iter_mut().enumerate() {
            p.order = u32::try_from(i).unwrap_or(u32::MAX);
        }
        let path = Self::config_path();
        match toml::to_string_pretty(self) {
            Ok(serialized) => {
                if let Err(e) = fs::write(&path, serialized) {
                    log::warn!("failed to write {}: {e}", path.display());
                }
            }
            Err(e) => log::warn!("failed to serialize iptv providers: {e}"),
        }
    }

    pub fn add(&mut self, provider: IptvProvider) {
        self.providers.push(provider);
        self.save();
    }

    pub fn update(&mut self, index: usize, provider: IptvProvider) {
        if let Some(slot) = self.providers.get_mut(index) {
            *slot = provider;
            self.save();
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.providers.len() {
            self.providers.remove(index);
            self.save();
        }
    }
}

// glib brought in via gtk re-export where available; import explicitly so
// this module compiles even without the broader UI imports.
use gtk::glib;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m3u_url_summary_is_the_url() {
        let p = IptvProvider::new_m3u_url("Foo", "https://example.com/list.m3u");
        assert_eq!(p.source.summary(), "https://example.com/list.m3u");
    }

    #[test]
    fn xtream_summary_hides_password() {
        let p = IptvProvider {
            name: "X".into(),
            source: IptvKind::Xtream {
                server: "https://srv.example".into(),
                username: "u".into(),
                password: "SECRET".into(),
            },
            user_agent: String::new(),
            referer: String::new(),
            order: 0,
        };
        let s = p.source.summary();
        assert!(!s.contains("SECRET"));
        assert!(s.contains("srv.example"));
    }

    #[test]
    fn roundtrip_toml() {
        let store = IptvProviderStore {
            providers: vec![
                IptvProvider::new_m3u_url("A", "https://a.example/list.m3u"),
                IptvProvider {
                    name: "B".into(),
                    source: IptvKind::Xtream {
                        server: "https://b.example".into(),
                        username: "u".into(),
                        password: "p".into(),
                    },
                    user_agent: "Mozilla/5.0 (X11)".into(),
                    referer: "https://b.example/".into(),
                    order: 1,
                },
            ],
        };
        let s = toml::to_string_pretty(&store).expect("serialize");
        let back: IptvProviderStore = toml::from_str(&s).expect("deserialize");
        assert_eq!(back.providers.len(), 2);
        assert_eq!(back.providers[0].name, "A");
        assert_eq!(back.providers[1].user_agent, "Mozilla/5.0 (X11)");
    }

    #[test]
    fn missing_optional_fields_default_to_empty() {
        let minimal = r#"
            [[providers]]
            name = "Bare"
            [providers.source]
            type = "m3u_url"
            url = "https://a.example/x.m3u"
        "#;
        let store: IptvProviderStore = toml::from_str(minimal).expect("deserialize");
        assert_eq!(store.providers.len(), 1);
        assert!(store.providers[0].user_agent.is_empty());
        assert!(store.providers[0].referer.is_empty());
    }
}
