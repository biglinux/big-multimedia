//! IPTV channel fetching: provider-id derivation + source resolution.
//!
//! Splits the synchronous, blocking work (HTTP / file read / M3U parse) out of
//! the [`VideoIptvBrowser`](super::VideoIptvBrowser) component so it can run on
//! a worker thread and so the pure `provider_identifier` mapping is unit
//! testable.

use crate::media::iptv::{
    IptvChannel, IptvKind, IptvProvider, MAX_PLAYLIST_BYTES, PlaylistEntries, decode_playlist,
    parse_channel_list, read_playlist_text,
};
use crate::video::ui::iptv::{cache, xtream};
use big_os_kit::http_client::{RequestHeaders, http_get_bytes_capped};
use big_os_kit::keyring::{SecretService, Vault};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// A whole provider list comes in one answer, tens of megabytes on a slow
/// panel: far longer than the default request time.
const PLAYLIST_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a provider's channels could not be loaded, so the person can act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// No answer: offline, name not found, timed out, refused.
    Unreachable,
    /// The server answered with this HTTP status.
    Status(u16),
    /// The list is larger than [`MAX_PLAYLIST_BYTES`].
    TooLarge,
    /// The answer holds no channels.
    Empty,
    /// The Xtream panel rejected the user name or password.
    LoginRejected,
    /// The Xtream account is not active; the panel's word for it.
    AccountInactive(String),
    /// The local playlist file could not be read.
    FileUnreadable,
    /// The keyring did not give back the provider's password.
    Keyring,
    /// A saved channel is on a server no Xtream provider is set up for.
    NoProvider,
}

/// A provider's channels, and the favorites to move to their stored URLs.
#[derive(Debug)]
pub struct FetchedChannels {
    pub channels: PlaylistEntries<IptvChannel>,
    /// `(played URL, stored URL)`: Xtream stars saved under the URL with the
    /// account's credentials in it.
    pub favorite_renames: Vec<(String, String)>,
}

/// The URLs of a channel about to play.
#[derive(Debug, PartialEq, Eq)]
pub struct ChannelUrls {
    /// What mpv opens, with the account's credentials in it.
    pub played: String,
    /// What the playlist lists and may save to a file; `None` when the played
    /// URL can carry the provider's secret in a form no stored URL marks.
    pub stored: Option<String>,
}

/// The URLs of the channel at `url` in `provider`'s list. Blocking, reads
/// the keyring: call from a worker thread.
///
/// # Errors
///
/// When the keyring does not give back the provider's secret.
pub fn channel_urls(
    provider: &IptvProvider,
    url: &str,
    vault: &dyn Vault,
) -> io::Result<ChannelUrls> {
    let unredacted = |text: &str| big_os_kit::url::redact_url(text) == text;
    Ok(match provider.resolved_source(vault)? {
        IptvKind::Xtream {
            username, password, ..
        } => ChannelUrls {
            played: xtream::stream_url(url, &username, &password),
            stored: (!xtream::shows_password(url, &password)).then(|| url.to_owned()),
        },
        IptvKind::M3uUrl { url: list } => ChannelUrls {
            played: url.to_owned(),
            stored: (unredacted(&list) && unredacted(url)).then(|| url.to_owned()),
        },
        IptvKind::M3uLocal { .. } => ChannelUrls {
            played: url.to_owned(),
            stored: unredacted(url).then(|| url.to_owned()),
        },
    })
}

/// `(stored, played)` for each of `entries` that names an Xtream account's
/// credentials. Blocking, reads the keyring once per provider.
///
/// # Errors
///
/// [`FetchError::NoProvider`] when no Xtream provider is on an entry's server,
/// [`FetchError::Keyring`] when the keyring keeps that provider's password.
pub fn played_channels(
    entries: &[String],
    providers: &[IptvProvider],
    vault: &dyn Vault,
) -> Result<Vec<(String, String)>, FetchError> {
    resolve_channels(entries, providers, vault, &|provider| {
        let origin = provider.source.list_origin();
        cache::read(&provider_identifier(provider))
            .map(|text| {
                parse_channel_list(&text, origin)
                    .entries
                    .into_iter()
                    .map(|channel| channel.url)
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// [`played_channels`] with each provider's stored channel URLs from
/// `line_up`, read only when several accounts share a server: the channel
/// goes to the account whose list carries it, else to the first one.
fn resolve_channels(
    entries: &[String],
    providers: &[IptvProvider],
    vault: &dyn Vault,
    line_up: &dyn Fn(&IptvProvider) -> HashSet<String>,
) -> Result<Vec<(String, String)>, FetchError> {
    let mut accounts: HashMap<usize, (String, String)> = HashMap::new();
    let mut line_ups: HashMap<usize, HashSet<String>> = HashMap::new();
    let mut channels = Vec::new();
    for entry in entries
        .iter()
        .filter(|entry| xtream::names_credentials(entry))
    {
        let serving: Vec<usize> = (0..providers.len())
            .filter(|&index| serves(&providers[index], entry))
            .collect();
        let &first = serving.first().ok_or(FetchError::NoProvider)?;
        let index = if serving.len() == 1 {
            first
        } else {
            serving
                .iter()
                .copied()
                .find(|&index| {
                    line_ups
                        .entry(index)
                        .or_insert_with(|| line_up(&providers[index]))
                        .contains(entry)
                })
                .unwrap_or(first)
        };
        let provider = &providers[index];
        let (username, password) = match accounts.entry(index) {
            Entry::Occupied(account) => account.into_mut(),
            Entry::Vacant(slot) => match provider.resolved_source(vault) {
                Ok(IptvKind::Xtream {
                    username, password, ..
                }) => slot.insert((username, password)),
                Ok(IptvKind::M3uUrl { .. } | IptvKind::M3uLocal { .. }) => {
                    return Err(FetchError::NoProvider);
                }
                Err(_) => return Err(FetchError::Keyring),
            },
        };
        channels.push((entry.clone(), xtream::stream_url(entry, username, password)));
    }
    Ok(channels)
}

/// Whether `url` is on the server of the Xtream `provider`, compared by
/// scheme, host and port with the scheme's default filled in: its
/// credentials go nowhere else.
fn serves(provider: &IptvProvider, url: &str) -> bool {
    let IptvKind::Xtream { server, .. } = &provider.source else {
        return false;
    };
    let flags = glib::UriFlags::PARSE_RELAXED | glib::UriFlags::ENCODED;
    let (Ok(server), Ok(url)) = (
        glib::Uri::parse(server, flags),
        glib::Uri::parse(url, flags),
    ) else {
        return false;
    };
    let port = |uri: &glib::Uri| match (uri.port(), uri.scheme().to_ascii_lowercase().as_str()) {
        (-1, "http") => 80,
        (-1, "https") => 443,
        (port, _) => port,
    };
    let host = |uri: &glib::Uri| uri.host().map(|host| host.to_ascii_lowercase());
    server.scheme().eq_ignore_ascii_case(&url.scheme())
        && host(&server).is_some()
        && host(&server) == host(&url)
        && port(&server) == port(&url)
        && url
            .path()
            .strip_prefix(server.path().trim_end_matches('/'))
            .is_some_and(|path| path.starts_with('/'))
}

/// Stable per-provider cache identifier; the cache names its file by a hash
/// of it.
#[must_use]
pub fn provider_identifier(provider: &IptvProvider) -> String {
    match &provider.source {
        IptvKind::M3uUrl { url } => format!("m3u_{}_{}", provider.name, url),
        IptvKind::M3uLocal { path } => format!("local_{}_{}", provider.name, path),
        IptvKind::Xtream {
            server, username, ..
        } => format!("xtream_{}_{}_{}", provider.name, server, username),
    }
}

/// GET a playlist or API answer as text, in UTF-8 or else Latin-1.
pub(super) fn fetch_text(url: &str, headers: &RequestHeaders<'_>) -> Result<String, FetchError> {
    let response = http_get_bytes_capped(url, headers, MAX_PLAYLIST_BYTES, PLAYLIST_TIMEOUT)
        .map_err(|error| {
            if error.contains("exceeded") {
                FetchError::TooLarge
            } else {
                FetchError::Unreachable
            }
        })?;
    if response.status != 200 {
        return Err(FetchError::Status(response.status));
    }
    Ok(decode_playlist(response.bytes))
}

/// Resolve a provider to its channel list over the network (or local file),
/// caching the raw playlist on success. Synchronous and blocking, reads the
/// keyring: call from a worker thread.
pub fn fetch_provider_channels(
    provider: &IptvProvider,
    provider_id: &str,
) -> Result<FetchedChannels, FetchError> {
    let headers = RequestHeaders {
        user_agent: &provider.user_agent,
        referer: &provider.referer,
    };
    let cancelled = AtomicBool::new(false);
    let source = provider
        .resolved_source(&SecretService {
            cancelled: &cancelled,
        })
        .map_err(|_| FetchError::Keyring)?;
    let content = match &source {
        IptvKind::M3uUrl { url } => fetch_text(url, &headers)?,
        IptvKind::M3uLocal { path } => {
            read_playlist_text(std::path::Path::new(path)).ok_or(FetchError::FileUnreadable)?
        }
        IptvKind::Xtream {
            server,
            username,
            password,
        } => xtream::fetch_xtream_m3u(server, username, password, &headers)?,
    };
    let channels = parse_channel_list(&content, provider.source.list_origin());
    if channels.entries.is_empty() && channels.skipped == 0 {
        return Err(FetchError::Empty);
    }
    if provider.lists_secret_urls() {
        cache::retire(provider_id);
    } else {
        cache::write(provider_id, &content);
    }
    let favorite_renames = match &source {
        IptvKind::Xtream {
            username, password, ..
        } => channels
            .entries
            .iter()
            .map(|channel| {
                let played = xtream::stream_url(&channel.url, username, password);
                (played, channel.url.clone())
            })
            .collect(),
        IptvKind::M3uUrl { .. } | IptvKind::M3uLocal { .. } => Vec::new(),
    };
    Ok(FetchedChannels {
        channels,
        favorite_renames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, source: IptvKind) -> IptvProvider {
        IptvProvider {
            name: name.to_string(),
            source,
            user_agent: String::new(),
            ..IptvProvider::default()
        }
    }

    /// A keyring holding one secret, or refusing every lookup.
    struct Keyring(Option<&'static str>);

    impl Vault for Keyring {
        fn store(&self, _: &str, _: &str, _: &[u8]) -> io::Result<()> {
            Err(io::Error::other("read-only"))
        }
        fn lookup(&self, _: &str, _: &str) -> io::Result<Vec<u8>> {
            self.0
                .map(|secret| secret.as_bytes().to_vec())
                .ok_or_else(|| io::Error::other("locked"))
        }
        fn clear(&self, _: &str, _: &str) -> io::Result<()> {
            Ok(())
        }
    }

    fn account(name: &str) -> IptvProvider {
        IptvProvider {
            secret_id: "item".into(),
            ..provider(
                name,
                IptvKind::Xtream {
                    server: "http://srv".into(),
                    username: "ana".into(),
                    password: String::new(),
                },
            )
        }
    }

    #[test]
    fn saved_playlists_hold_no_xtream_password() {
        let urls = channel_urls(
            &account("Panel"),
            "http://srv/live/{credentials}/7.ts",
            &Keyring(Some("s3c?ret")),
        )
        .unwrap();
        assert_eq!(urls.played, "http://srv/live/ana/s3c%3Fret/7.ts");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playlist.m3u8");
        let listed: Vec<String> = urls.stored.into_iter().collect();
        crate::media::playlist::write_m3u(&path, &listed).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            saved.contains("http://srv/live/{credentials}/7.ts"),
            "{saved}"
        );
        assert!(!saved.contains("s3c") && !saved.contains("ana"), "{saved}");
    }

    /// A list cached before its credentials were taken out, or an M3U URL
    /// whose secret the channel URLs may repeat, keeps the channel unlisted.
    #[test]
    fn secret_bearing_urls_stay_unlisted() {
        let cached = channel_urls(
            &account("Panel"),
            "http://srv/live/ana/s3c%3Fret/7.ts",
            &Keyring(Some("s3c?ret")),
        )
        .unwrap();
        assert_eq!(cached.stored, None);
        let list = IptvProvider {
            secret_id: "item".into(),
            ..provider(
                "List",
                IptvKind::M3uUrl {
                    url: "http://srv/get.php?username=ana&password=***".into(),
                },
            )
        };
        let keyring = Keyring(Some("http://srv/get.php?username=ana&password=s3cret"));
        let channel = channel_urls(&list, "http://srv/ana/s3cret/7", &keyring).unwrap();
        assert_eq!(channel.played, "http://srv/ana/s3cret/7");
        assert_eq!(channel.stored, None);
        // A public list, local or remote, can name a channel with a password.
        let channel_secret = "http://srv/7.ts?password=s3cret";
        for list in [
            IptvKind::M3uUrl {
                url: "http://srv/free.m3u".into(),
            },
            IptvKind::M3uLocal {
                path: "/home/ana/free.m3u".into(),
            },
        ] {
            let channel =
                channel_urls(&provider("Free", list), channel_secret, &Keyring(None)).unwrap();
            assert_eq!(channel.played, channel_secret);
            assert_eq!(channel.stored, None);
        }
    }

    #[test]
    fn saved_channels_play_with_their_provider_account() {
        let entries = [
            "/videos/a.mkv".to_string(),
            "http://srv/live/{credentials}/7.ts".to_string(),
            "http://srv/live/{credentials}/8.ts".to_string(),
        ];
        let providers = [
            provider(
                "Free",
                IptvKind::M3uUrl {
                    url: "http://srv/x.m3u".into(),
                },
            ),
            account("Panel"),
        ];
        let channels = played_channels(&entries, &providers, &Keyring(Some("pw"))).unwrap();
        assert_eq!(
            channels,
            [
                (
                    entries[1].clone(),
                    "http://srv/live/ana/pw/7.ts".to_string()
                ),
                (
                    entries[2].clone(),
                    "http://srv/live/ana/pw/8.ts".to_string()
                ),
            ]
        );
    }

    /// Two accounts on one server: the channel goes to the one whose list
    /// carries it, and to the first one when neither list does.
    #[test]
    fn shared_server_picks_listing_account() {
        let mut second = account("Second");
        if let IptvKind::Xtream { username, .. } = &mut second.source {
            *username = "bia".into();
        }
        let providers = [account("First"), second];
        let entries = ["http://srv/live/{credentials}/7.ts".to_string()];
        let listed_by_second = |provider: &IptvProvider| {
            if provider.name == "Second" {
                std::iter::once(entries[0].clone()).collect()
            } else {
                HashSet::new()
            }
        };
        let played = |line_up: &dyn Fn(&IptvProvider) -> HashSet<String>| {
            resolve_channels(&entries, &providers, &Keyring(Some("pw")), line_up)
                .unwrap()
                .remove(0)
                .1
        };
        assert_eq!(played(&listed_by_second), "http://srv/live/bia/pw/7.ts");
        assert_eq!(played(&|_| HashSet::new()), "http://srv/live/ana/pw/7.ts");
    }

    #[test]
    fn locked_keyring_refuses_saved_channels() {
        let entries = ["http://srv/live/{credentials}/7.ts".to_string()];
        assert_eq!(
            played_channels(&entries, &[account("Panel")], &Keyring(None)),
            Err(FetchError::Keyring)
        );
    }

    /// A playlist file names the server; an account goes only to its own.
    #[test]
    fn credentials_never_go_to_another_server() {
        for entry in [
            "http://srv.example/live/{credentials}/7.ts",
            "http://other/live/{credentials}/7.ts",
            "https://srv/live/{credentials}/7.ts",
            "http://srv:8080/live/{credentials}/7.ts",
        ] {
            assert_eq!(
                played_channels(
                    &[entry.to_string()],
                    &[account("Panel")],
                    &Keyring(Some("pw"))
                ),
                Err(FetchError::NoProvider),
                "{entry}"
            );
        }
    }

    /// RFC 3986, 6.2.3: a scheme's default port names the same server.
    #[test]
    fn default_port_names_the_same_server() {
        let mut panel = account("Panel");
        if let IptvKind::Xtream { server, .. } = &mut panel.source {
            *server = "HTTP://SRV:80/".into();
        }
        let entries = ["http://srv/live/{credentials}/7.ts".to_string()];
        assert_eq!(
            played_channels(&entries, &[panel], &Keyring(Some("pw"))),
            Ok(vec![(
                entries[0].clone(),
                "http://srv/live/ana/pw/7.ts".to_string()
            )])
        );
    }

    #[test]
    fn secretless_lists_list_played_urls() {
        let free = provider(
            "Free",
            IptvKind::M3uUrl {
                url: "http://a/x.m3u".into(),
            },
        );
        let urls = channel_urls(&free, "http://a/1.ts", &Keyring(None)).unwrap();
        assert_eq!(urls.stored.as_deref(), Some(urls.played.as_str()));
    }

    #[test]
    fn identifier_m3u_url() {
        let p = provider(
            "Free",
            IptvKind::M3uUrl {
                url: "http://a/x.m3u".into(),
            },
        );
        assert_eq!(provider_identifier(&p), "m3u_Free_http://a/x.m3u");
    }

    #[test]
    fn identifier_local() {
        let p = provider(
            "Disk",
            IptvKind::M3uLocal {
                path: "/home/u/list.m3u".into(),
            },
        );
        assert_eq!(provider_identifier(&p), "local_Disk_/home/u/list.m3u");
    }

    #[test]
    fn identifier_xtream_omits_password() {
        let p = provider(
            "Panel",
            IptvKind::Xtream {
                server: "http://srv".into(),
                username: "user".into(),
                password: "SECRET".into(),
            },
        );
        let id = provider_identifier(&p);
        assert_eq!(id, "xtream_Panel_http://srv_user");
        assert!(!id.contains("SECRET"));
    }
}
