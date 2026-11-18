//! One request head in, one response out: parsing, the token route, the
//! origin and rate checks, and the JSON and static replies.

use big_os_kit::http_client::validate_header_name;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{RemoteHandlers, StatusSnapshot};

const MAX_HEADERS: usize = 48;
const MUTATIONS: [&str; 4] = ["play-pause", "next", "prev", "volume"];

/// Answers one complete request head. `admit` charges the mutation rate budget.
pub(super) fn respond(
    input: &[u8],
    token: &str,
    handlers: &RemoteHandlers,
    stop: &AtomicBool,
    admit: impl FnOnce() -> bool,
) -> Vec<u8> {
    let end = input.windows(4).position(|w| w == b"\r\n\r\n");
    let Some(head) = end
        .filter(|end| end + 4 == input.len())
        .map(|end| &input[..end])
    else {
        return response(
            400,
            "text/plain",
            "Pipelining and request bodies are not supported",
        );
    };
    match parse_request(head) {
        Ok(request) => dispatch(&request, token, handlers, stop, admit),
        Err(()) => response(400, "text/plain", "Invalid HTTP request"),
    }
}

struct Request<'a> {
    method: &'a str,
    target: &'a str,
    headers: HashMap<String, &'a str>,
}
fn parse_request(input: &[u8]) -> Result<Request<'_>, ()> {
    if !input.is_ascii() {
        return Err(());
    }
    let text = std::str::from_utf8(input).map_err(|_| ())?;
    let mut lines = text.split("\r\n");
    let first = lines.next().ok_or(())?;
    let mut parts = first.split(' ');
    let method = parts.next().ok_or(())?;
    let target = parts.next().ok_or(())?;
    if parts.next() != Some("HTTP/1.1")
        || parts.next().is_some()
        || !matches!(method, "GET" | "POST")
        || !target.starts_with('/')
        || target.len() > 2048
        || target.bytes().any(|b| b <= 32 || b == 127)
    {
        return Err(());
    }
    let mut headers = HashMap::new();
    for line in lines {
        let (key, value) = line.split_once(':').ok_or(())?;
        if validate_header_name(key).is_err()
            || value.bytes().any(|b| (b < 32 && b != b'\t') || b == 127)
        {
            return Err(());
        }
        if headers.len() >= MAX_HEADERS
            || headers
                .insert(key.to_ascii_lowercase(), value.trim())
                .is_some()
        {
            return Err(());
        }
    }
    let host = headers.get("host").ok_or(())?;
    if host.is_empty() || host.contains(['/', '@', ' ', '\t', '#', '?']) {
        return Err(());
    }
    if headers.contains_key("transfer-encoding")
        || headers.contains_key("expect")
        || headers
            .get("content-length")
            .is_some_and(|length| *length != "0")
    {
        return Err(());
    }
    Ok(Request {
        method,
        target,
        headers,
    })
}

/// Returns what follows `/<token>` in `target`, comparing the token in constant time.
fn route<'a>(target: &'a str, token: &str) -> Option<&'a str> {
    let (candidate, rest) = target.strip_prefix('/')?.split_at_checked(token.len())?;
    let diff = candidate
        .bytes()
        .zip(token.bytes())
        .fold(0, |diff, (a, b)| diff | (a ^ b));
    (diff == 0).then_some(rest)
}

fn dispatch(
    request: &Request<'_>,
    token: &str,
    handlers: &RemoteHandlers,
    stop: &AtomicBool,
    admit: impl FnOnce() -> bool,
) -> Vec<u8> {
    if stop.load(Ordering::Acquire) {
        return response(503, "text/plain", "Remote disabled");
    }
    let Some(rest) = route(request.target, token) else {
        return response(404, "text/plain", "Not Found");
    };
    if rest.is_empty() && request.method == "GET" {
        return format!("HTTP/1.1 308 Permanent Redirect\r\nLocation: /{token}/\r\nContent-Length: 0\r\n{COMMON_HEADERS}\r\n").into_bytes();
    }
    let Some(tail) = rest.strip_prefix('/') else {
        return response(404, "text/plain", "Not Found");
    };
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    let mutation = MUTATIONS.contains(&path);
    if mutation != (request.method == "POST") {
        return response(405, "text/plain", "Method Not Allowed");
    }
    let headers = &request.headers;
    let cross_origin = headers
        .get("origin")
        .is_some_and(|origin| !origin.eq_ignore_ascii_case(&format!("http://{}", headers["host"])))
        || headers
            .get("sec-fetch-site")
            .is_some_and(|site| !matches!(*site, "same-origin" | "none"));
    if cross_origin || (mutation && headers.get("x-big-media-remote") != Some(&"1")) {
        return response(403, "text/plain", "Forbidden");
    }
    if mutation && !admit() {
        return response(429, "text/plain", "Too Many Requests");
    }
    match path {
        "" | "index.html" => response(200, "text/html; charset=utf-8", INDEX_HTML),
        "remote.css" => response(200, "text/css; charset=utf-8", INDEX_CSS),
        "remote.js" => response(200, "text/javascript; charset=utf-8", INDEX_JS),
        "status" => response(
            200,
            "application/json; charset=utf-8",
            &status_json(&(handlers.status)()),
        ),
        "play-pause" => command_response((handlers.play_pause)()),
        "next" => command_response((handlers.next)()),
        "prev" => command_response((handlers.prev)()),
        "volume" => match parse_volume(query) {
            Some(volume) => command_response((handlers.set_volume)(volume)),
            None => response(400, "text/plain", "Invalid volume"),
        },
        _ => response(404, "text/plain", "Not Found"),
    }
}
fn command_response(accepted: bool) -> Vec<u8> {
    if accepted {
        response(200, "text/plain", "ok")
    } else {
        response(503, "text/plain", "Player busy or remote disabled; retry")
    }
}

const COMMON_HEADERS: &str = "Connection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'\r\n";

fn response(status: u16, mime: &str, body: &str) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        429 => "Too Many Requests",
        _ => "Service Unavailable",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n{COMMON_HEADERS}\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn status_json(status: &StatusSnapshot) -> String {
    let finite = |value: f64| {
        if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        }
    };
    serde_json::json!({
        "playing": status.playing,
        "title": status.title.chars().take(2048).collect::<String>(),
        "artist": status.artist.chars().take(2048).collect::<String>(),
        "position": finite(status.position_s),
        "duration": finite(status.duration_s),
        "volume": finite(status.volume).min(100.0),
    })
    .to_string()
}

fn parse_volume(query: &str) -> Option<f64> {
    let mut volumes = query.split('&').filter_map(|pair| pair.strip_prefix("v="));
    let value: f64 = volumes.next()?.parse().ok()?;
    if volumes.next().is_some() || !value.is_finite() {
        return None;
    }
    Some(value.clamp(0.0, 100.0))
}
const INDEX_HTML: &str = include_str!("remote_control.html");
const INDEX_CSS: &str = include_str!("remote_control.css");
const INDEX_JS: &str = include_str!("remote_control.js");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_control::tests::{counting_handlers, handlers, status_code};
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn volume_parse_clamps() {
        assert_eq!(parse_volume("v=50"), Some(50.0));
        assert_eq!(parse_volume("v=150"), Some(100.0));
        assert_eq!(parse_volume("v=-5"), Some(0.0));
        assert_eq!(parse_volume("foo=bar&v=42.5"), Some(42.5));
        assert_eq!(parse_volume("nope"), None);
        assert_eq!(parse_volume("v=NaN"), None);
        assert_eq!(parse_volume("v=2&v=3"), None);
    }

    #[test]
    fn status_json_escapes_text() {
        let status_snapshot = StatusSnapshot {
            playing: true,
            title: "A \"quoted\" title\n\u{1f}".into(),
            artist: "Artist\\Name".into(),
            position_s: f64::NAN,
            duration_s: 98.5,
            volume: 142.0,
        };

        let value: serde_json::Value =
            serde_json::from_str(&status_json(&status_snapshot)).unwrap();

        assert_eq!(value["playing"], true);
        assert_eq!(value["title"], "A \"quoted\" title\n\u{1f}");
        assert_eq!(value["artist"], "Artist\\Name");
        assert_eq!(value["position"], 0.0);
        assert_eq!(value["duration"], 98.5);
        assert_eq!(value["volume"], 100.0);
    }

    #[test]
    fn routes_require_exact_token() {
        assert_eq!(route("/secret/status", "secret"), Some("/status"));
        assert_eq!(route("/secret", "secret"), Some(""));
        assert_eq!(route("/secreX/status", "secret"), None);
        assert_eq!(route("/secr", "secret"), None);
        let request = |target: &str| {
            let head = format!("GET {target} HTTP/1.1\r\nHost: localhost");
            dispatch(
                &parse_request(head.as_bytes()).unwrap(),
                "secret",
                &handlers(true),
                &AtomicBool::new(false),
                || true,
            )
        };
        assert!(request("/secret-extra/status").starts_with(b"HTTP/1.1 404"));
        assert!(request("/secret").starts_with(b"HTTP/1.1 308"));
        assert!(request("/secret/status").starts_with(b"HTTP/1.1 200"));
    }

    #[test]
    fn parser_rejects_smuggling_and_duplicates() {
        for input in [
            "POST /token/next HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1",
            "POST /token/next HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nContent-Length: 0",
            "POST /token/next HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked",
            "GET /token/status HTTP/1.1\r\nHost: localhost\r\n folded:value",
            "GET /token/status HTTP/1.1\r\nHost: localhost\r\n:value",
            "GET /token/status HTTP/1.1\r\nHost: localhost\r\nExpect: 100-continue",
        ] {
            assert!(parse_request(input.as_bytes()).is_err(), "{input}");
        }
    }

    /// Status, handler calls and whether the rate budget was charged.
    fn exchange(head: &str, admitted: bool) -> (String, usize, bool) {
        let calls = Arc::new(AtomicUsize::new(0));
        let charged = std::cell::Cell::new(false);
        let request = format!("{head}\r\nHost: localhost\r\n\r\n");
        let reply = respond(
            request.as_bytes(),
            "t",
            &counting_handlers(&calls),
            &AtomicBool::new(false),
            || {
                charged.set(true);
                admitted
            },
        );
        let handled = calls.load(Ordering::SeqCst);
        (status_code(&reply).to_owned(), handled, charged.get())
    }

    #[test]
    fn refusals_skip_handlers() {
        for head in [
            "GET /t/status HTTP/1.1\r\nOrigin: http://evil",
            "POST /t/next HTTP/1.1\r\nOrigin: http://evil\r\nX-Big-Media-Remote: 1",
            "GET /t/status HTTP/1.1\r\nSec-Fetch-Site: cross-site",
            "POST /t/next HTTP/1.1\r\nSec-Fetch-Site: same-site\r\nX-Big-Media-Remote: 1",
            "POST /t/next HTTP/1.1",
            "POST /t/volume?v=5 HTTP/1.1\r\nX-Big-Media-Remote: 0",
        ] {
            assert_eq!(exchange(head, true), ("403".into(), 0, false), "{head}");
        }
        let marked = "POST /t/play-pause HTTP/1.1\r\nX-Big-Media-Remote: 1";
        assert_eq!(exchange(marked, false), ("429".into(), 0, true));
        let same_origin = "POST /t/next HTTP/1.1\r\nOrigin: http://localhost\r\n\
                           Sec-Fetch-Site: same-origin\r\nX-Big-Media-Remote: 1";
        assert_eq!(exchange(same_origin, true), ("200".into(), 1, true));
    }

    #[test]
    fn busy_handler_returns_503() {
        let req =
            parse_request(b"POST /token/next HTTP/1.1\r\nHost: localhost\r\nX-Big-Media-Remote: 1")
                .unwrap();
        let reply = dispatch(
            &req,
            "token",
            &handlers(false),
            &AtomicBool::new(false),
            || true,
        );
        assert!(reply.starts_with(b"HTTP/1.1 503"));
    }
}
