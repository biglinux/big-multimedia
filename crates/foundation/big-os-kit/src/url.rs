//! URL helpers: which schemes a media target may use, RFC 3986 percent-encoding
//! of query values, and redaction for logs.

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

/// Network stream schemes a list written by someone else may name. mpv opens
/// anything else it knows too (`av://lavfi`, `fdclose://`, `memory://`), which
/// writes files or closes the player's descriptors.
const STREAM_SCHEMES: [&str; 10] = [
    "http", "https", "rtsp", "rtsps", "rtmp", "rtmps", "mms", "udp", "rtp", "srt",
];

/// Paths no list entry may resolve into: a crafted playlist naming them probes
/// for files through mpv's errors or its subtitle and cover loaders.
const SYSTEM_PATH_DENY: [&str; 6] = ["/etc", "/proc", "/sys", "/dev", "/root", "/boot"];

fn has_scheme_of<'a>(value: &'a str, schemes: &[&str]) -> Option<&'a str> {
    let (scheme, rest) = value.split_once("://")?;
    schemes
        .iter()
        .any(|known| scheme.eq_ignore_ascii_case(known))
        .then_some(rest)
}

/// An `http://` or `https://` URL with a host, in any letter case.
#[must_use]
pub fn is_http_url(value: &str) -> bool {
    has_scheme_of(value, &["http", "https"])
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
}

/// A URL of one of the network stream schemes mpv plays (HTTP, RTSP, RTMP,
/// MMS, UDP, RTP, SRT and their TLS forms), in any letter case.
#[must_use]
pub fn is_stream_uri(value: &str) -> bool {
    has_scheme_of(value, &STREAM_SCHEMES).is_some()
}

/// Where a list of media entries came from, which decides what it may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListOrigin {
    /// A feed, an IPTV list or a playlist fetched over the network.
    Network,
    /// A playlist file on this computer.
    LocalFile,
}

/// Whether a list from `origin` may name `entry`, without touching the disk: a
/// stream URI, or an absolute path when the list itself is a local file.
#[must_use]
pub fn is_list_target(entry: &str, origin: ListOrigin) -> bool {
    is_stream_uri(entry) || (origin == ListOrigin::LocalFile && entry.starts_with('/'))
}

/// [`is_list_target`], and a path must resolve, after `..` and symlinks, to an
/// existing file outside the system directories. Blocking.
#[must_use]
pub fn accepts_list_entry(entry: &str, origin: ListOrigin) -> bool {
    if !is_list_target(entry, origin) {
        return false;
    }
    if is_stream_uri(entry) {
        return true;
    }
    std::fs::canonicalize(entry)
        .is_ok_and(|path| !SYSTEM_PATH_DENY.iter().any(|deny| path.starts_with(deny)))
}

/// The local path a `file://` URI names (empty or `localhost` authority),
/// percent-decoded. `None` for any other URI, a decoded NUL or an escaped `/`.
#[must_use]
pub fn file_uri_path(uri: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = has_scheme_of(uri, &["file"])?;
    let path = if rest.starts_with('/') {
        rest
    } else {
        let slash = rest.find('/')?;
        if !rest[..slash].eq_ignore_ascii_case("localhost") {
            return None;
        }
        &rest[slash..]
    };
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            // An escaped slash would join two path components, as GLib refuses.
            let byte = high * 16 + low;
            if byte == b'/' {
                return None;
            }
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(byte);
            index += 1;
        }
    }
    if decoded.contains(&0) {
        return None;
    }
    Some(std::ffi::OsString::from_vec(decoded).into())
}

/// `value` as a path on this computer: an absolute path as written, or the
/// [`file_uri_path`] of a `file://` URI. `None` for anything else, a relative
/// path included.
#[must_use]
pub fn local_path(value: &str) -> Option<std::path::PathBuf> {
    if value.starts_with('/') {
        Some(value.into())
    } else {
        file_uri_path(value)
    }
}

/// URI path without authority, query or fragment. Does not decode path bytes.
/// A bare authority such as https://radio.mp3 has an empty path, not an MP3 file.
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

/// Privacy-first diagnostic wrapper. It logs only the scheme (or
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
/// `SENSITIVE_QUERY_KEYS`, e.g. Xtream `username`/`password`) become `***`.
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

/// Longest name [`filename_from_url_path`] returns, leaving room under the
/// usual 255-byte limit for a suffix the caller adds.
const MAX_FILENAME_BYTES: usize = 200;
/// Longest final extension that truncation keeps.
const MAX_KEPT_EXTENSION_BYTES: usize = 16;

/// Return the final URL path segment when it looks like a filename, else
/// `fallback_filename`. A segment without a `.`, with a leading `.` or with a
/// NUL is refused; the name is cut to 200 bytes, keeping a short extension.
#[must_use]
pub fn filename_from_url_path(url: &str, fallback_filename: &str) -> String {
    let segment = uri_path(url).rsplit('/').next().unwrap_or_default();
    // A leading dot hides the file or aliases "." and "..".
    let name = if segment.contains('.') && !segment.starts_with('.') && !segment.contains('\0') {
        segment
    } else {
        fallback_filename
    };
    if name.len() <= MAX_FILENAME_BYTES {
        return name.to_string();
    }
    let extension = name
        .rfind('.')
        .map(|dot| &name[dot..])
        .filter(|extension| extension.len() <= MAX_KEPT_EXTENSION_BYTES)
        .unwrap_or_default();
    let stem_end = name.floor_char_boundary(MAX_FILENAME_BYTES - extension.len());
    format!("{}{extension}", &name[..stem_end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Xorshift step, reduced to a small index range for picking from pools.
    fn next_random(state: &mut u64) -> usize {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        usize::try_from(*state % 1024).unwrap()
    }

    /// Reference decoder for the oracle: `%XX` becomes the byte, anything else stays.
    fn percent_decode(encoded: &str) -> Vec<u8> {
        let bytes = encoded.as_bytes();
        let mut decoded = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%' {
                let pair = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap();
                decoded.push(u8::from_str_radix(pair, 16).unwrap());
                index += 3;
            } else {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
        decoded
    }

    #[test]
    fn encoded_query_value_decodes_back() {
        let pool = [
            'a', 'Z', '0', '-', '_', '.', '~', ' ', '%', '&', '=', '+', '?', '#', '/', 'é', '東',
            '🎵', '\0', '\u{7f}',
        ];
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for _ in 0..5000 {
            let length = next_random(&mut state) % 40;
            let value: String = (0..length)
                .map(|_| pool[next_random(&mut state) % pool.len()])
                .collect();
            let encoded = encode_query_value(&value);
            let bytes = encoded.as_bytes();
            let mut index = 0;
            while index < bytes.len() {
                if bytes[index] == b'%' {
                    assert!(
                        bytes[index + 1..index + 3]
                            .iter()
                            .all(|digit| matches!(digit, b'0'..=b'9' | b'A'..=b'F')),
                        "{value:?} gave {encoded:?}"
                    );
                    index += 3;
                } else {
                    assert!(
                        bytes[index].is_ascii_alphanumeric() || b"-_.~".contains(&bytes[index]),
                        "{value:?} gave {encoded:?}"
                    );
                    index += 1;
                }
            }
            assert_eq!(percent_decode(&encoded), value.as_bytes(), "{encoded:?}");
        }
    }

    /// Spells `key` in random letter case with random bytes percent-encoded.
    fn disguised_key(key: &str, state: &mut u64) -> String {
        let mut disguised = String::new();
        for byte in key.bytes() {
            let byte = if next_random(state).is_multiple_of(2) {
                byte.to_ascii_uppercase()
            } else {
                byte
            };
            if next_random(state).is_multiple_of(3) {
                disguised.push_str(&format!("%{byte:02x}"));
            } else {
                disguised.push(char::from(byte));
            }
        }
        disguised
    }

    #[test]
    fn redaction_hides_disguised_secrets() {
        let mut state = 0x853c_49e6_748f_ea9b_u64;
        for round in 0..2000 {
            let mut url = String::from("https://host/path?page=2");
            let mut secrets = Vec::new();
            for pair in 0..1 + next_random(&mut state) % 4 {
                let key =
                    SENSITIVE_QUERY_KEYS[next_random(&mut state) % SENSITIVE_QUERY_KEYS.len()];
                let secret = format!("Qz{round}x{pair}y{}", next_random(&mut state));
                url.push_str(&format!("&{}={secret}", disguised_key(key, &mut state)));
                secrets.push(secret);
            }
            let redacted = redact_url(&url);
            for secret in &secrets {
                assert!(!redacted.contains(secret.as_str()), "{url} gave {redacted}");
            }
            assert!(redacted.contains("page=2"), "{url} gave {redacted}");
        }
    }

    #[test]
    fn redaction_is_idempotent() {
        let pool = [
            "https://", "h", "/", ":", "@", "?", "&", "=", "#", "%", "%74", "token", "Pass", "***",
            "é",
        ];
        let mut state = 0xda94_2042_e4dd_58b5_u64;
        for _ in 0..5000 {
            let length = next_random(&mut state) % 24;
            let url: String = (0..length)
                .map(|_| pool[next_random(&mut state) % pool.len()])
                .collect();
            let once = redact_url(&url);
            assert_eq!(redact_url(&once), once, "{url:?}");
        }
    }

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
    fn filename_is_last_path_segment() {
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
    fn redact_masks_keys_case_insensitively() {
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
    fn long_filename_keeps_extension() {
        let url = format!("https://h/{}.mp3", "a".repeat(300));
        assert_eq!(
            filename_from_url_path(&url, "fallback.mp3"),
            format!("{}.mp3", "a".repeat(196))
        );
        // 196 bytes left for the stem hold 65 three-byte characters.
        let fallback = format!("{}.mp3", "東".repeat(100));
        assert_eq!(
            filename_from_url_path("https://h/stream", &fallback),
            format!("{}.mp3", "東".repeat(65))
        );
        assert_eq!(
            filename_from_url_path("https://h/.hidden.mp3", "fallback.mp3"),
            "fallback.mp3"
        );
    }

    #[test]
    fn url_filename_is_one_short_component() {
        let pool = [
            'a', 'É', '東', '🎵', '.', '/', '\0', ' ', '%', '?', '#', '\u{301}',
        ];
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || next_random(&mut state);
        for _ in 0..5000 {
            let length = next() % 400;
            let path: String = (0..length).map(|_| pool[next() % pool.len()]).collect();
            let name = filename_from_url_path(&format!("https://h/{path}"), "fallback.mp3");
            assert!(
                !name.is_empty()
                    && name != "."
                    && name != ".."
                    && !name.contains(['/', '\0'])
                    && name.len() <= 200,
                "{path:?} gave {name:?}"
            );
        }
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
    fn stream_and_http_schemes() {
        // (value, is_http_url, is_stream_uri)
        for (value, http, stream) in [
            ("https://h/a.mp3", true, true),
            ("HTTP://h", true, true),
            ("http://", false, true),
            ("https:///etc/passwd", false, true),
            ("http:h/a", false, false),
            ("rtsps://cam/live", false, true),
            ("RTMP://h/app", false, true),
            ("udp://@239.0.0.1:1234", false, true),
            ("srt://h:9000", false, true),
            ("mms://h/s", false, true),
            ("av://lavfi:sine", false, false),
            ("fdclose://3", false, false),
            ("memory://x", false, false),
            ("file:///music/a.mp3", false, false),
            ("dvd://", false, false),
            ("/music/a.mp3", false, false),
            ("/music/http://a", false, false),
            ("", false, false),
        ] {
            assert_eq!(
                (is_http_url(value), is_stream_uri(value)),
                (http, stream),
                "{value:?}"
            );
        }
    }

    #[test]
    fn list_entries_follow_their_origin() {
        let dir = std::env::temp_dir().join(format!("list-entry-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let song = dir.join("song.mp3");
        std::fs::write(&song, b"").unwrap();
        let link = dir.join("hostname.mp3");
        std::os::unix::fs::symlink("/etc/hostname", &link).unwrap();
        let song = song.to_str().unwrap();
        let link = link.to_str().unwrap();
        let up = format!("{}/../../../../../../../../etc/passwd", dir.display());
        let missing = format!("{}/missing.mp3", dir.display());
        // (entry, from a local file, from the network)
        for (entry, local, network) in [
            ("https://h/a.mp3", true, true),
            ("srt://h:9000", true, true),
            ("av://lavfi:sine,ametadata=mode=print:file=/x", false, false),
            ("fdclose://0", false, false),
            ("file:///music/a.mp3", false, false),
            ("song.mp3", false, false),
            (song, true, false),
            (link, false, false),
            (up.as_str(), false, false),
            (missing.as_str(), false, false),
            ("/proc/self/environ", false, false),
            ("/dev/stdin", false, false),
        ] {
            assert_eq!(
                (
                    accepts_list_entry(entry, ListOrigin::LocalFile),
                    accepts_list_entry(entry, ListOrigin::Network)
                ),
                (local, network),
                "{entry:?}"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn file_uri_decodes_to_a_path() {
        for (uri, path) in [
            ("file:///music/a%20b.mp3", Some("/music/a b.mp3")),
            ("FILE://localhost/m/%C3%A9.ogg", Some("/m/é.ogg")),
            ("file://host/m/a.mp3", None),
            ("file:///m/a%2Fb", None),
            ("file:///m/a%00b", None),
            ("file:///m/a%zz", None),
            ("file:///m/a%2", None),
            ("https://h/a", None),
            ("/m/a.mp3", None),
        ] {
            assert_eq!(
                file_uri_path(uri).as_deref(),
                path.map(std::path::Path::new),
                "{uri}"
            );
        }
    }

    #[test]
    fn local_path_takes_absolute_paths_and_file_uris() {
        for (value, path) in [
            ("/m/100%25.mp3", Some("/m/100%25.mp3")),
            ("file:///m/a%20b.mp3", Some("/m/a b.mp3")),
            ("file://host/m/a.mp3", None),
            ("m/a.mp3", None),
            ("https://h/a.mp3", None),
            ("", None),
        ] {
            assert_eq!(
                local_path(value).as_deref(),
                path.map(std::path::Path::new),
                "{value}"
            );
        }
    }

    #[test]
    fn authority_yields_no_filename() {
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
