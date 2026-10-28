//! IPTV channel fetching: provider-id derivation + source resolution.
//!
//! Splits the synchronous, blocking work (HTTP / file read / M3U parse) out of
//! the [`VideoIptvBrowser`](super::VideoIptvBrowser) component so it can run on
//! a worker thread and so the pure `provider_identifier` mapping is unit
//! testable.

use crate::media::iptv::{
    IptvChannel, IptvKind, IptvProvider, MAX_PLAYLIST_BYTES, decode_playlist, parse_m3u_content,
    read_playlist_text,
};
use crate::video::ui::iptv::{cache, xtream};
use big_os_kit::http_client::{RequestHeaders, http_get_bytes_capped};
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
}

/// Stable per-provider cache identifier. Fed through `cache::write`'s slugifier
/// downstream, so any unsafe characters get sanitized there.
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
/// caching the raw playlist on success. Synchronous and blocking: call from a
/// worker thread.
pub fn fetch_provider_channels(
    provider: &IptvProvider,
    provider_id: &str,
) -> Result<Vec<IptvChannel>, FetchError> {
    let headers = RequestHeaders {
        user_agent: &provider.user_agent,
        referer: &provider.referer,
    };
    let content = match &provider.source {
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
    let channels = parse_m3u_content(&content);
    if channels.is_empty() {
        return Err(FetchError::Empty);
    }
    cache::write(provider_id, &content);
    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(name: &str, source: IptvKind) -> IptvProvider {
        IptvProvider {
            name: name.to_string(),
            source,
            user_agent: String::new(),
            referer: String::new(),
            order: 0,
        }
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
