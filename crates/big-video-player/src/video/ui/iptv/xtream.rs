//! Minimal Xtream Codes API client (Relm4 port).
//!
//! Xtream Codes is a de-facto commercial IPTV panel format used by most paid
//! providers. Rather than talk the full API (categories, VOD, series, EPG), we
//! take the shortcut most IPTV players do: request the panel's built-in
//! "get.php" endpoint which exports the entire TV line-up as a single M3U
//! document. That means:
//!
//! - One request returns everything (with `type=m3u_plus&output=m3u8`).
//! - The resulting M3U carries `group-title` attributes, so the regular parser
//!   already handles grouping.
//! - Users get live channels immediately; VOD/series support (full API) is a
//!   later-phase enhancement.
//!
//! If a provider rejects `get.php`, the caller gets `None` and should surface a
//! diagnostic. Most Xtream panels support it. Engine-free helper; mirrors the
//! legacy `iptv_view::xtream` until the V6 cutover deletes it.

use big_app_kit::http_client::{RequestHeaders, http_get_with_headers};
use big_app_kit::url::encode_query_value as urlencode;

/// Build the `get.php` URL for a Xtream account and fetch the full M3U.
///
/// `server` must include the scheme (`http://` / `https://`). Trailing slashes
/// are tolerated. The credentials are URL-encoded (untrusted user input).
/// Returns the raw M3U text on a 2xx with a non-empty body, `None` on any
/// failure. The helper is sync; call it from a worker thread.
#[must_use]
pub fn fetch_xtream_m3u(
    server: &str,
    username: &str,
    password: &str,
    headers: &RequestHeaders<'_>,
) -> Option<String> {
    let url = xtream_get_php_url(server, username, password);
    let body = http_get_with_headers(&url, headers)?;
    if body.trim().is_empty() {
        return None;
    }
    Some(body)
}

/// Build the `get.php` request URL for a Xtream account (URL-encoding the
/// credentials). Split out for unit testing without a live network call.
#[must_use]
pub fn xtream_get_php_url(server: &str, username: &str, password: &str) -> String {
    let base = server.trim_end_matches('/');
    format!(
        "{base}/get.php?username={}&password={}&type=m3u_plus&output=m3u8",
        urlencode(username),
        urlencode(password)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencodes_special_chars() {
        assert_eq!(urlencode("user@example"), "user%40example");
        assert_eq!(urlencode("pa&ss"), "pa%26ss");
        assert_eq!(urlencode("plain123-_.~"), "plain123-_.~");
    }

    #[test]
    fn urlencodes_plus_and_equal() {
        assert_eq!(urlencode("a+b=c"), "a%2Bb%3Dc");
    }

    #[test]
    fn urlencodes_empty_string() {
        assert_eq!(urlencode(""), "");
    }

    #[test]
    fn get_php_url_trims_trailing_slash_and_encodes() {
        let url = xtream_get_php_url("http://srv.example/", "u@x", "p&w");
        assert_eq!(
            url,
            "http://srv.example/get.php?username=u%40x&password=p%26w&type=m3u_plus&output=m3u8"
        );
    }

    #[test]
    fn get_php_url_keeps_scheme_and_host() {
        let url = xtream_get_php_url("https://host.tld", "user", "pass");
        assert!(url.starts_with("https://host.tld/get.php?"));
        assert!(url.contains("type=m3u_plus"));
    }
}
