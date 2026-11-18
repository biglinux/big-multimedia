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

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Source kinds understood by the IPTV subsystem.
///
/// `M3uUrl` / `M3uLocal` cover the vast majority of real-world usage;
/// `Xtream` adds support for the Xtream Codes API used by commercial
/// panels (category listing, VOD, series).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl std::fmt::Debug for IptvKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::M3uUrl { .. } => "M3uUrl(<redacted>)",
            Self::M3uLocal { .. } => "M3uLocal(<path>)",
            Self::Xtream { .. } => "Xtream(<redacted>)",
        })
    }
}

impl IptvKind {
    /// Human-readable single-line summary for the management dialog.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            IptvKind::M3uUrl { url } => big_os_kit::url::redact_url(url),
            IptvKind::M3uLocal { path } => path.clone(),
            IptvKind::Xtream {
                server, username, ..
            } => format!("{} ({username})", big_os_kit::url::redact_url(server)),
        }
    }

    /// Where this source's channel list comes from.
    #[must_use]
    pub fn list_origin(&self) -> big_os_kit::url::ListOrigin {
        match self {
            IptvKind::M3uLocal { .. } => big_os_kit::url::ListOrigin::LocalFile,
            IptvKind::M3uUrl { .. } | IptvKind::Xtream { .. } => {
                big_os_kit::url::ListOrigin::Network
            }
        }
    }
}

/// One configured IPTV source.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl std::fmt::Debug for IptvProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IptvProvider")
            .field("source", &self.source)
            .finish_non_exhaustive()
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

/// On-disk provider container. A missing file may start empty; a corrupt file
/// blocks persistence and is surfaced through save_status instead of being erased.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IptvProviderStore {
    #[serde(default)]
    pub providers: Vec<IptvProvider>,
    #[serde(skip)]
    persistence: Option<big_os_kit::deferred::Document>,
}

impl IptvProviderStore {
    /// Location only: no directories are created on the GUI thread.
    pub fn config_path() -> PathBuf {
        big_os_kit::xdg_dirs::config_dir()
            .map(|p| {
                p.join(crate::config::video::CONFIG_DIR)
                    .join("iptv-providers.toml")
            })
            .unwrap_or_default()
    }

    /// Read bounded state. Corruption is retained as a blocked writer, never
    /// interpreted as permission to replace the original with an empty store.
    #[must_use]
    pub fn load() -> Self {
        use big_os_kit::{deferred::Document, storage::read_regular_limited};
        let path = Self::config_path();
        let (mut store, writer) = match read_regular_limited(&path, big_os_kit::deferred::MAX_BYTES)
        {
            Ok(None) => (Self::default(), Document::reviewed(path, None)),
            Ok(Some(bytes)) => {
                let decoded = std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|text| toml::from_str::<Self>(text).ok());
                match decoded {
                    Some(store) if store.providers.len() <= 256 => {
                        (store, Document::reviewed(path, Some(bytes)))
                    }
                    _ => (Self::default(), Document::blocked(path)),
                }
            }
            Err(_) => (Self::default(), Document::blocked(path)),
        };
        store.persistence = Some(writer);
        store
    }

    /// Save admission only. Acknowledged disk state is exposed by `save_status`.
    pub fn save(&mut self) {
        let writer = self.persistence.get_or_insert_with(|| {
            big_os_kit::deferred::Document::reviewed(Self::config_path(), None)
        });
        if self.providers.len() > 256 {
            writer.reject_update();
            log::warn!("too many IPTV providers; not saved");
            return;
        }
        for (i, provider) in self.providers.iter_mut().enumerate() {
            provider.order = i as u32;
        }
        let writer = writer.clone();
        if let Ok(bytes) = toml::to_string_pretty(self) {
            if writer.replace(bytes.into_bytes()).is_err() {
                log::warn!("IPTV provider preferences not saved; inspect save status");
            }
        } else {
            writer.reject_update();
            log::warn!("IPTV provider encoding failed");
        }
    }
    /// Observable result; no credential bytes are exposed.
    pub fn save_status(&self) -> Option<big_os_kit::deferred::Status> {
        self.persistence
            .as_ref()
            .map(big_os_kit::deferred::Document::status)
    }
    /// Retry a pending durability failure without overwriting an external change.
    pub fn retry_save(&self) {
        if let Some(p) = &self.persistence {
            let _ = p.retry();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn m3u_url_provider(name: &str, url: &str) -> IptvProvider {
        IptvProvider {
            name: name.into(),
            source: IptvKind::M3uUrl { url: url.into() },
            ..IptvProvider::default()
        }
    }

    #[test]
    fn m3u_url_summary_is_the_url() {
        let p = m3u_url_provider("Foo", "https://example.com/list.m3u");
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

    /// Any text, line breaks and control characters included.
    const ANY_TEXT: &str = "(?s).{0,30}";

    fn any_provider() -> impl proptest::strategy::Strategy<Value = IptvProvider> {
        use proptest::prelude::*;
        let source = prop_oneof![
            ANY_TEXT.prop_map(|url| IptvKind::M3uUrl { url }),
            ANY_TEXT.prop_map(|path| IptvKind::M3uLocal { path }),
            (ANY_TEXT, ANY_TEXT, ANY_TEXT).prop_map(|(server, username, password)| {
                IptvKind::Xtream {
                    server,
                    username,
                    password,
                }
            }),
        ];
        (ANY_TEXT, source, ANY_TEXT, ANY_TEXT, any::<u32>()).prop_map(
            |(name, source, user_agent, referer, order)| IptvProvider {
                name,
                source,
                user_agent,
                referer,
                order,
            },
        )
    }

    proptest::proptest! {
        #[test]
        fn store_roundtrips_through_toml(
            providers in proptest::collection::vec(any_provider(), 0..5)
        ) {
            let store = IptvProviderStore { providers, ..IptvProviderStore::default() };
            let text = toml::to_string_pretty(&store).expect("serialize");
            let back: IptvProviderStore = toml::from_str(&text).expect("deserialize");
            proptest::prop_assert_eq!(back.providers, store.providers);
        }
    }

    /// The file a user keeps between versions: its `type` tags and field names
    /// are the format contract.
    #[test]
    fn fixture_file_loads_every_kind() {
        let text = include_str!("../../../tests/fixtures/iptv-providers.toml");
        let store: IptvProviderStore = toml::from_str(text).expect("deserialize");
        let expected = vec![
            IptvProvider {
                name: "Free List".into(),
                source: IptvKind::M3uUrl {
                    url: "https://lists.example/free.m3u8".into(),
                },
                ..IptvProvider::default()
            },
            IptvProvider {
                name: "Saved".into(),
                source: IptvKind::M3uLocal {
                    path: "/home/ana/Videos/channels.m3u".into(),
                },
                order: 1,
                ..IptvProvider::default()
            },
            IptvProvider {
                name: "Panel".into(),
                source: IptvKind::Xtream {
                    server: "http://panel.example:8080".into(),
                    username: "ana".into(),
                    password: "s3cret".into(),
                },
                user_agent: "VLC/3.0.20".into(),
                referer: "https://panel.example/".into(),
                order: 2,
            },
        ];
        assert_eq!(store.providers, expected);
    }
}
