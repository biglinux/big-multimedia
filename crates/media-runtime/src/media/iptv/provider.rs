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
//! Passwords are not in it: an Xtream password, or an M3U URL that carries
//! credentials, lives in the Secret Service and the file names its item.

use std::io;
use std::path::PathBuf;

use big_os_kit::keyring::Vault;
use serde::{Deserialize, Serialize};

/// Keyring scope of the provider secrets.
pub const SECRET_SCOPE: &str = "iptv";

/// Source kinds understood by the IPTV subsystem.
///
/// `M3uUrl` / `M3uLocal` cover the vast majority of real-world usage;
/// `Xtream` adds support for the Xtream Codes API used by commercial
/// panels (category listing, VOD, series).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IptvKind {
    /// Remote M3U/M3U8 URL fetched over HTTP(S). One that carries credentials
    /// is in the keyring, and `url` holds its redacted form.
    M3uUrl { url: String },
    /// Local path to an M3U/M3U8 file on disk.
    M3uLocal { path: String },
    /// Xtream Codes server. `password` is empty once it is in the keyring.
    Xtream {
        server: String,
        username: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
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
    /// Keyring item, in [`SECRET_SCOPE`], holding the Xtream password or the
    /// M3U URL with its credentials.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret_id: String,
}

impl IptvProvider {
    /// The secret this provider holds in plain text: a typed Xtream password or
    /// one read from an older file, or an M3U URL that carries credentials.
    #[must_use]
    pub fn plaintext_secret(&self) -> Option<&str> {
        match &self.source {
            IptvKind::Xtream { password, .. } if !password.is_empty() => Some(password),
            IptvKind::M3uUrl { url } if big_os_kit::url::redact_url(url) != *url => Some(url),
            IptvKind::M3uUrl { .. } | IptvKind::M3uLocal { .. } | IptvKind::Xtream { .. } => None,
        }
    }

    /// Move the plain-text secret into `vault` under a fresh item and keep
    /// only its reference. Blocking.
    ///
    /// # Errors
    ///
    /// When no id can be drawn or `vault` refuses the item; the provider is
    /// then unchanged.
    pub fn protect_secret(&mut self, vault: &dyn Vault) -> io::Result<()> {
        let Some(secret) = self.plaintext_secret() else {
            return Ok(());
        };
        let id = big_os_kit::random::random_hex_id()?;
        vault.store(SECRET_SCOPE, &id, secret.as_bytes())?;
        match &mut self.source {
            IptvKind::Xtream { password, .. } => password.clear(),
            IptvKind::M3uUrl { url } => *url = big_os_kit::url::redact_url(url),
            IptvKind::M3uLocal { .. } => {}
        }
        self.secret_id = id;
        Ok(())
    }

    /// The source with its secret read back from `vault`. Blocking.
    ///
    /// # Errors
    ///
    /// When `vault` cannot return the item: locked, unavailable or deleted.
    pub fn resolved_source(&self, vault: &dyn Vault) -> io::Result<IptvKind> {
        let mut source = self.source.clone();
        if self.secret_id.is_empty() {
            return Ok(source);
        }
        let secret = String::from_utf8(vault.lookup(SECRET_SCOPE, &self.secret_id)?)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "keyring item is not text"))?;
        match &mut source {
            IptvKind::Xtream { password, .. } => *password = secret,
            IptvKind::M3uUrl { url } => *url = secret,
            IptvKind::M3uLocal { .. } => {}
        }
        Ok(source)
    }
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
            secret_id: String::new(),
        }
    }
}

const MAX_PROVIDERS: usize = 256;

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
        let (mut store, writer) =
            big_os_kit::deferred::Document::load(Self::config_path(), |bytes| {
                let text = std::str::from_utf8(bytes).ok()?;
                toml::from_str::<Self>(text)
                    .ok()
                    .filter(|store| store.providers.len() <= MAX_PROVIDERS)
            });
        store.persistence = Some(writer);
        store
    }

    /// Save admission only. Acknowledged disk state is exposed by `save_status`.
    pub fn save(&mut self) {
        let writer = self.persistence.get_or_insert_with(|| {
            big_os_kit::deferred::Document::reviewed(Self::config_path(), None)
        });
        if self.providers.len() > MAX_PROVIDERS {
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

    /// Copies of the providers with a secret in plain text, by index.
    #[must_use]
    pub fn unprotected(&self) -> Vec<(usize, IptvProvider)> {
        self.providers
            .iter()
            .enumerate()
            .filter(|(_, provider)| provider.plaintext_secret().is_some())
            .map(|(index, provider)| (index, provider.clone()))
            .collect()
    }

    /// Put back each `(index, original, protected)` whose slot still holds
    /// `original`, so an edit made meanwhile wins. True when any changed; the
    /// caller saves.
    pub fn apply_protected(&mut self, protected: Vec<(usize, IptvProvider, IptvProvider)>) -> bool {
        let mut changed = false;
        for (index, original, provider) in protected {
            if let Some(slot) = self.providers.get_mut(index)
                && *slot == original
            {
                *slot = provider;
                changed = true;
            }
        }
        changed
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
            ..IptvProvider::default()
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
        (ANY_TEXT, source, ANY_TEXT, ANY_TEXT, any::<u32>(), ANY_TEXT).prop_map(
            |(name, source, user_agent, referer, order, secret_id)| IptvProvider {
                name,
                source,
                user_agent,
                referer,
                order,
                secret_id,
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
                    password: String::new(),
                },
                user_agent: "VLC/3.0.20".into(),
                referer: "https://panel.example/".into(),
                order: 2,
                secret_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            },
        ];
        assert_eq!(store.providers, expected);
        assert!(store.unprotected().is_empty());
    }

    /// Secret Service stand-in: items by (scope, id), or refusing every call
    /// as a locked keyring does.
    #[derive(Default)]
    struct MemoryVault {
        items: std::cell::RefCell<std::collections::BTreeMap<(String, String), Vec<u8>>>,
        locked: bool,
    }

    impl Vault for MemoryVault {
        fn store(&self, scope: &str, id: &str, value: &[u8]) -> io::Result<()> {
            if self.locked {
                return Err(io::Error::other("locked"));
            }
            self.items
                .borrow_mut()
                .insert((scope.into(), id.into()), value.to_vec());
            Ok(())
        }
        fn lookup(&self, scope: &str, id: &str) -> io::Result<Vec<u8>> {
            if self.locked {
                return Err(io::Error::other("locked"));
            }
            self.items
                .borrow()
                .get(&(scope.into(), id.into()))
                .cloned()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        }
        fn clear(&self, scope: &str, id: &str) -> io::Result<()> {
            self.items.borrow_mut().remove(&(scope.into(), id.into()));
            Ok(())
        }
    }

    fn xtream(password: &str) -> IptvProvider {
        IptvProvider {
            name: "Panel".into(),
            source: IptvKind::Xtream {
                server: "http://panel.example:8080".into(),
                username: "ana".into(),
                password: password.into(),
            },
            ..IptvProvider::default()
        }
    }

    fn saved_text(providers: Vec<IptvProvider>) -> String {
        let store = IptvProviderStore {
            providers,
            ..IptvProviderStore::default()
        };
        toml::to_string_pretty(&store).expect("serialize")
    }

    #[test]
    fn xtream_password_moves_into_the_keyring() {
        let vault = MemoryVault::default();
        let mut provider = xtream("s3cret");
        provider.protect_secret(&vault).unwrap();

        assert_eq!(provider.plaintext_secret(), None);
        assert_eq!(provider.secret_id.len(), 32);
        let text = saved_text(vec![provider.clone()]);
        assert!(!text.contains("s3cret"), "{text}");
        assert!(!text.contains("password"), "{text}");
        assert!(text.contains(&provider.secret_id), "{text}");
        let items = vault.items.borrow();
        assert_eq!(
            items.get(&(SECRET_SCOPE.into(), provider.secret_id.clone())),
            Some(&b"s3cret".to_vec())
        );
        drop(items);
        assert_eq!(
            provider.resolved_source(&vault).unwrap(),
            xtream("s3cret").source
        );
    }

    #[test]
    fn credentialed_m3u_url_moves_to_keyring() {
        let full = "https://lists.example/get.php?username=ana&password=s3cret&type=m3u";
        let vault = MemoryVault::default();
        let mut provider = m3u_url_provider("List", full);
        provider.protect_secret(&vault).unwrap();

        let text = saved_text(vec![provider.clone()]);
        assert!(!text.contains("s3cret"), "{text}");
        assert!(!text.contains("username=ana"), "{text}");
        assert!(text.contains("lists.example/get.php"), "{text}");
        assert_eq!(provider.plaintext_secret(), None);
        assert_eq!(
            provider.resolved_source(&vault).unwrap(),
            IptvKind::M3uUrl { url: full.into() }
        );
    }

    #[test]
    fn sources_without_credentials_stay_in_the_file() {
        let vault = MemoryVault::default();
        for mut provider in [
            m3u_url_provider("Free", "https://lists.example/free.m3u8"),
            xtream(""),
            IptvProvider {
                source: IptvKind::M3uLocal {
                    path: "/home/ana/list.m3u".into(),
                },
                ..IptvProvider::default()
            },
        ] {
            let before = provider.clone();
            provider.protect_secret(&vault).unwrap();
            assert_eq!(provider, before);
            assert_eq!(provider.resolved_source(&vault).unwrap(), before.source);
        }
        assert!(vault.items.borrow().is_empty());
    }

    #[test]
    fn locked_keyring_changes_nothing() {
        let locked = MemoryVault {
            locked: true,
            ..MemoryVault::default()
        };
        let mut provider = xtream("s3cret");
        assert!(provider.protect_secret(&locked).is_err());
        assert_eq!(provider, xtream("s3cret"));

        provider.protect_secret(&MemoryVault::default()).unwrap();
        assert!(provider.resolved_source(&locked).is_err());
    }

    /// A file written before the keyring held the secrets.
    #[test]
    fn plaintext_file_loses_its_secrets() {
        let text = r#"
            [[providers]]
            name = "Panel"
            [providers.source]
            type = "xtream"
            server = "http://panel.example:8080"
            username = "ana"
            password = "s3cret"

            [[providers]]
            name = "Free"
            [providers.source]
            type = "m3u_url"
            url = "https://lists.example/free.m3u8"
        "#;
        let mut store: IptvProviderStore = toml::from_str(text).expect("deserialize");
        let unprotected = store.unprotected();
        assert_eq!(unprotected.len(), 1);

        let vault = MemoryVault::default();
        let protected = unprotected
            .into_iter()
            .map(|(index, original)| {
                let mut provider = original.clone();
                provider.protect_secret(&vault).unwrap();
                (index, original, provider)
            })
            .collect();
        assert!(store.apply_protected(protected));

        let rewritten = toml::to_string_pretty(&store).expect("serialize");
        assert!(!rewritten.contains("s3cret"), "{rewritten}");
        assert!(store.unprotected().is_empty());
        assert_eq!(store.providers.len(), 2);
        assert_eq!(
            store.providers[0].resolved_source(&vault).unwrap(),
            xtream("s3cret").source
        );
    }

    #[test]
    fn protecting_skips_a_provider_edited_meanwhile() {
        let mut store = IptvProviderStore {
            providers: vec![xtream("old")],
            ..IptvProviderStore::default()
        };
        let (index, original) = store.unprotected().remove(0);
        store.providers[0] = xtream("typed meanwhile");
        let mut protected = original.clone();
        protected.protect_secret(&MemoryVault::default()).unwrap();

        assert!(!store.apply_protected(vec![(index, original, protected)]));
        assert_eq!(store.providers[0], xtream("typed meanwhile"));
    }
}
