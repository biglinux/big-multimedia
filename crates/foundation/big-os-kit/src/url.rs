//! RFC 3986 percent-encoding helpers for URL query values.

/// Query-parameter keys (matched case-insensitively) whose values [`redact_url`]
/// masks in the compatibility redactor. Covers Xtream Codes credentials
/// (`username`/`password`) and common token/secret parameter names.
const SENSITIVE_QUERY_KEYS: &[&str] = &[
    "password",
    "pass",
    "pwd",
    "username",
    "user",
    "token",
    "access_token",
    "secret",
    "api_key",
    "apikey",
    "key",
    "auth",
    "session",
    "sid",
    "sig",
    "signature",
    "credential",
    "refresh_token",
    "id_token",
    "x-amz-credential",
    "x-amz-signature",
    "x-amz-security-token",
    "x-goog-credential",
    "x-goog-signature",
    "policy",
    "key-pair-id",
];

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
fn is_sensitive_query_key(key: &str) -> bool {
    // Bound decoding work. Malformed/oversize encoded keys are treated as
    // sensitive rather than creating a redaction bypass.
    if key.len() > 512 {
        return true;
    }
    let mut decoded = Vec::with_capacity(key.len());
    let bytes = key.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(value) = bytes
                .get(index + 1)
                .and_then(|a| hex(*a))
                .zip(bytes.get(index + 2).and_then(|b| hex(*b)))
            else {
                return true;
            };
            decoded.push(value.0 * 16 + value.1);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    SENSITIVE_QUERY_KEYS
        .iter()
        .any(|sensitive| decoded.eq_ignore_ascii_case(sensitive.as_bytes()))
}

/// Syntactic RFC 3986 scheme, without allocating or changing the original URI.
#[must_use]
pub fn uri_scheme(value: &str) -> Option<&str> {
    let (scheme, _) = value.split_once(':')?;
    let mut bytes = scheme.bytes();
    if !bytes.next().is_some_and(|c| c.is_ascii_alphabetic())
        || !bytes.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
    {
        return None;
    }
    Some(scheme)
}

/// URI path without authority, query or fragment. Does not decode path bytes.
/// A bare authority such as https://radio.mp3 has an EMPTY path, not an MP3 file.
#[must_use]
pub fn uri_path(value: &str) -> &str {
    let clean = &value[..value.find(['?', '#']).unwrap_or(value.len())];
    let Some(scheme) = uri_scheme(clean) else {
        return clean;
    };
    let rest = &clean[scheme.len() + 1..];
    if let Some(authority) = rest.strip_prefix("//") {
        authority.find('/').map_or("", |slash| &authority[slash..])
    } else {
        rest
    }
}

/// Privacy-first diagnostic wrapper. It intentionally logs only the scheme (or
/// 'local media'), never the path, authority, query or fragment, which may all
/// carry provider-specific credentials. The playback target itself is untouched.
pub struct MediaLogTarget<'a>(pub &'a str);
impl std::fmt::Display for MediaLogTarget<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match uri_scheme(self.0).filter(|s| s.len() <= 32) {
            Some(scheme) => write!(f, "{scheme} media"),
            None => f.write_str("local media"),
        }
    }
}

/// Mask the `userinfo` (`user:pass@`) component of a URL authority, if present.
fn redact_userinfo(base: &str) -> String {
    let Some(scheme_end) = base.find("://") else {
        return base.to_string();
    };
    let authority_start = scheme_end + 3;
    let rest = &base[authority_start..];
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    match authority.rfind('@') {
        Some(at) => format!(
            "{}***@{}{}",
            &base[..authority_start],
            &authority[at + 1..],
            &rest[authority_end..]
        ),
        None => base.to_string(),
    }
}

/// Best-effort known-parameter redaction (not a universal log sanitizer): the authority `userinfo`
/// (`user:pass@`) and the values of sensitive query parameters (see
/// `SENSITIVE_QUERY_KEYS` — e.g. Xtream `username`/`password`) become `***`.
/// Best-effort string rewrite that never fails; a non-URL string is returned
/// unchanged apart from any `key=value` query pairs it recognizes.
#[must_use]
pub fn redact_url(url: &str) -> String {
    // Fragments often contain OAuth responses. Do not retain arbitrary fragment
    // data in this compatibility redactor.
    let (url, fragment) = url
        .split_once('#')
        .map_or((url, false), |(base, _)| (base, true));
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };

    let mut out = redact_userinfo(base);

    if let Some(query) = query {
        out.push('?');
        for (index, pair) in query.split('&').enumerate() {
            if index > 0 {
                out.push('&');
            }
            match pair.split_once('=') {
                Some((key, value)) if !value.is_empty() && is_sensitive_query_key(key) => {
                    out.push_str(key);
                    out.push_str("=***");
                }
                _ => out.push_str(pair),
            }
        }
    }
    if fragment {
        out.push_str("#***");
    }
    out
}

/// Percent-encode a query value while preserving the RFC 3986 unreserved set.
#[must_use]
pub fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Return the final URL path segment when it looks like a filename.
#[must_use]
pub fn filename_from_url_path(url: &str, fallback_filename: &str) -> String {
    uri_path(url)
        .rsplit('/')
        .next()
        .and_then(|segment| {
            let name = segment.split('?').next().unwrap_or(segment);
            // "." and ".." would escape or alias the download directory.
            if !name.contains('.') || name.trim_matches('.').is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .unwrap_or_else(|| fallback_filename.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_value_keeps_unreserved_ascii() {
        assert_eq!(encode_query_value("A-Z_0.9~"), "A-Z_0.9~");
    }

    #[test]
    fn query_value_encodes_spaces_and_reserved_chars() {
        assert_eq!(encode_query_value("a b&c=d?/"), "a%20b%26c%3Dd%3F%2F");
    }

    #[test]
    fn query_value_encodes_utf8_bytes() {
        assert_eq!(encode_query_value("café"), "caf%C3%A9");
        assert_eq!(encode_query_value("東京"), "%E6%9D%B1%E4%BA%AC");
    }

    #[test]
    fn filename_from_url_path_keeps_last_segment_without_query() {
        assert_eq!(
            filename_from_url_path("https://example.test/a/b/song.flac?token=1", "fallback.mp3"),
            "song.flac"
        );
    }

    #[test]
    fn redact_masks_xtream_username_and_password() {
        assert_eq!(
            redact_url("http://host:8080/get.php?username=joe&password=hunter2&type=m3u_plus"),
            "http://host:8080/get.php?username=***&password=***&type=m3u_plus"
        );
    }

    #[test]
    fn redact_masks_userinfo() {
        assert_eq!(
            redact_url("http://joe:hunter2@host:8080/live/stream.ts"),
            "http://***@host:8080/live/stream.ts"
        );
    }

    #[test]
    fn redact_masks_token_secret_and_key_case_insensitively() {
        assert_eq!(
            redact_url("https://api.test/v1?API_KEY=abc&Token=xyz&page=2"),
            "https://api.test/v1?API_KEY=***&Token=***&page=2"
        );
    }

    #[test]
    fn redact_leaves_clean_urls_untouched() {
        let clean = "https://example.test/a/b/song.flac?format=json&page=3";
        assert_eq!(redact_url(clean), clean);
    }

    #[test]
    fn redact_keeps_empty_sensitive_values_empty() {
        assert_eq!(
            redact_url("http://host/get.php?password=&type=m3u"),
            "http://host/get.php?password=&type=m3u"
        );
    }

    #[test]
    fn filename_from_url_path_requires_extension() {
        assert_eq!(
            filename_from_url_path("https://example.test/a/stream", "fallback.mp3"),
            "fallback.mp3"
        );
        assert_eq!(
            filename_from_url_path("https://example.test/a/", "fallback.mp3"),
            "fallback.mp3"
        );
    }
    #[test]
    fn encoded_secrets_and_fragments_are_redacted() {
        assert_eq!(
            redact_url("https://h/a?%74oken=secret&X-Amz-Signature=proof#access_token=hidden"),
            "https://h/a?%74oken=***&X-Amz-Signature=***#***"
        );
        assert_eq!(
            format!(
                "{}",
                MediaLogTarget("https://user:secret@h/path-token?q=secret")
            ),
            "https media"
        );
    }
    #[test]
    fn authority_is_not_a_filename() {
        assert_eq!(uri_path("https://radio.mp3"), "");
        assert_eq!(
            uri_path("HTTPS://radio/episode.MP3?sig=x#part"),
            "/episode.MP3"
        );
        assert_eq!(
            filename_from_url_path("https://radio.mp3", "fallback"),
            "fallback"
        );
    }
}
