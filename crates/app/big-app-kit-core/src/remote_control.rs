//! HTTP server for token-scoped local web remotes.
//!
//! Serves a single-page HTML control surface plus a tiny JSON API:
//!
//! ```text
//! GET  /<token>/            -> remote_control.html
//! GET  /<token>/status      -> current playback snapshot (JSON)
//! POST /<token>/play-pause  -> toggle
//! POST /<token>/next
//! POST /<token>/prev
//! POST /<token>/volume?v=75 -> 0..100
//! ```
//!
//! Requests outside `/<token>/...` are answered with 404 — the token is
//! per-session and shared with the phone only through the QR code, so LAN
//! neighbors cannot guess the URL.

use log::info;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Current playback snapshot. Handlers must return quickly from memory, never I/O.
pub struct StatusSnapshot {
    /// Confirmed playback status.
    pub playing: bool,
    /// Current title.
    pub title: String,
    /// Current artist.
    pub artist: String,
    /// Confirmed position, seconds.
    pub position_s: f64,
    /// Duration, seconds; zero if unknown.
    pub duration_s: f64,
    /// Volume percent.
    pub volume: f64,
}
/// Actions enqueue non-blocking messages onto the app's owning thread and return
/// whether bounded admission succeeded. True is not confirmation of playback.
/// Slow network peers never call these until their request is fully validated.
pub struct RemoteHandlers {
    /// Toggle playback.
    pub play_pause: Arc<dyn Fn() -> bool + Send + Sync>,
    /// Next track.
    pub next: Arc<dyn Fn() -> bool + Send + Sync>,
    /// Previous track.
    pub prev: Arc<dyn Fn() -> bool + Send + Sync>,
    /// Set bounded finite volume.
    pub set_volume: Arc<dyn Fn(f64) -> bool + Send + Sync>,
    /// Read in-memory state.
    pub status: Arc<dyn Fn() -> StatusSnapshot + Send + Sync>,
}
/// Owner of a LAN-only control service. Drop revokes dispatch and wakes shutdown.
pub struct ServerHandle {
    /// Bound TCP port.
    pub port: u16,
    /// Fresh random capability for this server lifetime.
    pub token: String,
    stop: Arc<AtomicBool>,
    wake: Option<thread::Thread>,
}
impl ServerHandle {
    /// Revoke new dispatch immediately; drain no network uploads or writes.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(wake) = &self.wake {
            wake.unpark();
        }
    }
}
impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

const MAX_CLIENTS: usize = 16;
const MAX_PER_IP: usize = 4;
const MAX_HEADER: usize = 8192;
const MAX_HEADERS: usize = 48;
const HEADER_DEADLINE: Duration = Duration::from_secs(3);
const WRITE_DEADLINE: Duration = Duration::from_secs(2);
const MAX_RATE_PEERS: usize = 64;

/// Start with the same revocation token carried by queued UI commands. `stop`
/// must be fresh and false; a stopped session is never reused for another token.
pub fn start_scoped(
    address: SocketAddr,
    handlers: RemoteHandlers,
    stop: Arc<AtomicBool>,
) -> Result<ServerHandle, String> {
    if stop.load(Ordering::Acquire) {
        return Err("Remote session already revoked".into());
    }
    let token = make_token().map_err(|_| "Secure randomness unavailable".to_owned())?;
    let listener = TcpListener::bind(address).map_err(|e| format!("bind: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("nonblocking: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("address: {e}"))?
        .port();
    let flag = stop.clone();
    let secret = token.clone();
    let worker = thread::Builder::new()
        .name("media-remote".into())
        .spawn(move || {
            serve(listener, &secret, &handlers, &flag);
        })
        .map_err(|_| "Could not start remote control worker".to_owned())?;
    let wake = Some(worker.thread().clone()); // no blocking Join in GTK Drop
    info!("web-remote listening on port {port}");
    Ok(ServerHandle {
        port,
        token,
        stop,
        wake,
    })
}

struct Client {
    stream: TcpStream,
    ip: IpAddr,
    accepted: Instant,
    input: Vec<u8>,
    output: Option<Vec<u8>>,
    sent: usize,
    write_started: Option<Instant>,
}
struct Rate {
    start: Instant,
    count: u32,
}
impl Rate {
    fn allow(&mut self, now: Instant, cap: u32) -> bool {
        if now.duration_since(self.start) >= Duration::from_secs(1) {
            self.start = now;
            self.count = 0;
        }
        if self.count >= cap {
            return false;
        }
        self.count += 1;
        true
    }
}
fn serve(listener: TcpListener, token: &str, handlers: &RemoteHandlers, stop: &AtomicBool) {
    let mut clients: Vec<Client> = Vec::with_capacity(MAX_CLIENTS);
    let mut rates: HashMap<IpAddr, Rate> = HashMap::new();
    let mut global = Rate {
        start: Instant::now(),
        count: 0,
    };
    while !stop.load(Ordering::Acquire) {
        // Finite accept budget per iteration; floods cannot starve existing peers.
        for _ in 0..MAX_CLIENTS {
            match listener.accept() {
                Ok((stream, peer)) => {
                    if clients.len() >= MAX_CLIENTS
                        || clients.iter().filter(|c| c.ip == peer.ip()).count() >= MAX_PER_IP
                    {
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }
                    if stream.set_nonblocking(true).is_err() {
                        continue;
                    }
                    clients.push(Client {
                        stream,
                        ip: peer.ip(),
                        accepted: Instant::now(),
                        input: Vec::with_capacity(1024),
                        output: None,
                        sent: 0,
                        write_started: None,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    stop.store(true, Ordering::Release);
                    break;
                }
            }
        }
        let now = Instant::now();
        rates.retain(|_, rate| now.duration_since(rate.start) < Duration::from_secs(10));
        clients.retain_mut(|client| {
            if stop.load(Ordering::Acquire) {
                return false;
            }
            if client.output.is_none() {
                // Absolute deadline from accept, not reset by one byte per second.
                if now.duration_since(client.accepted) >= HEADER_DEADLINE {
                    return false;
                }
                let mut chunk = [0u8; 2048];
                match client.stream.read(&mut chunk) {
                    Ok(0) => return false,
                    Ok(n) => {
                        if client.input.len() + n > MAX_HEADER {
                            return false;
                        }
                        client.input.extend_from_slice(&chunk[..n]);
                        if let Some(end) = client.input.windows(4).position(|w| w == b"\r\n\r\n") {
                            let response = if end + 4 != client.input.len() {
                                response(
                                    400,
                                    "text/plain",
                                    "Pipelining and request bodies are not supported",
                                )
                            } else {
                                match parse_request(&client.input[..end]) {
                                    Ok(request) => {
                                        let mutation = rate_limited_mutation(&request, token);
                                        let permit = if mutation {
                                            if !rates.contains_key(&client.ip)
                                                && rates.len() >= MAX_RATE_PEERS
                                            {
                                                false
                                            } else {
                                                rates
                                                    .entry(client.ip)
                                                    .or_insert(Rate {
                                                        start: now,
                                                        count: 0,
                                                    })
                                                    .allow(now, 8)
                                                    && global.allow(now, 24)
                                            }
                                        } else {
                                            true
                                        };
                                        dispatch(&request, token, handlers, stop, permit)
                                    }
                                    Err(()) => response(400, "text/plain", "Invalid HTTP request"),
                                }
                            };
                            client.output = Some(response);
                            client.write_started = Some(Instant::now());
                            client.input.clear();
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return false,
                }
            }
            if let Some(output) = &client.output {
                if client
                    .write_started
                    .is_some_and(|start| start.elapsed() >= WRITE_DEADLINE)
                {
                    return false;
                }
                let end = (client.sent + 8192).min(output.len());
                match client.stream.write(&output[client.sent..end]) {
                    Ok(0) => return false,
                    Ok(n) => {
                        client.sent += n;
                        if client.sent == output.len() {
                            return false;
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return false,
                }
            }
            true
        });
        thread::park_timeout(if clients.is_empty() {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(10)
        });
    }
    for client in clients {
        let _ = client.stream.shutdown(Shutdown::Both);
    }
    drop(listener);
    info!("web-remote stopped");
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
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
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
fn rate_limited_mutation(request: &Request<'_>, token: &str) -> bool {
    let Some(tail) = route(request.target, token) else {
        return false;
    };
    let path = tail.split_once('?').map_or(tail, |(path, _)| path);
    request.method == "POST"
        && matches!(path, "play-pause" | "next" | "prev" | "volume")
        && request.headers.get("x-big-media-remote") == Some(&"1")
        && request.headers.get("origin").is_none_or(|origin| {
            origin.eq_ignore_ascii_case(&format!("http://{}", request.headers["host"]))
        })
        && request
            .headers
            .get("sec-fetch-site")
            .is_none_or(|site| matches!(*site, "same-origin" | "none"))
}
fn route<'a>(url: &'a str, token: &str) -> Option<&'a str> {
    url.strip_prefix(&format!("/{token}/"))
}
fn dispatch(
    request: &Request<'_>,
    token: &str,
    handlers: &RemoteHandlers,
    stop: &AtomicBool,
    permit: bool,
) -> Vec<u8> {
    if stop.load(Ordering::Acquire) {
        return response(503, "text/plain", "Remote disabled");
    }
    if request.target == format!("/{token}") && request.method == "GET" {
        let mut bytes = response(308, "text/plain", "");
        // Insert only a generated random-token path, never user data.
        let insert = format!("Location: /{token}/\r\n");
        let pos = bytes.windows(2).position(|w| w == b"\r\n").unwrap_or(0) + 2;
        bytes.splice(pos..pos, insert.bytes());
        return bytes;
    }
    let Some(tail) = route(request.target, token) else {
        return response(404, "text/plain", "Not Found");
    };
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    let mutation = matches!(path, "play-pause" | "next" | "prev" | "volume");
    if (mutation && request.method != "POST") || (!mutation && request.method != "GET") {
        return response(405, "text/plain", "Method Not Allowed");
    }
    let headers = &request.headers;
    let origin_ok = headers
        .get("origin")
        .is_none_or(|origin| origin.eq_ignore_ascii_case(&format!("http://{}", headers["host"])));
    if !origin_ok
        || headers
            .get("sec-fetch-site")
            .is_some_and(|site| !matches!(*site, "same-origin" | "none"))
        || (mutation && headers.get("x-big-media-remote") != Some(&"1"))
    {
        return response(403, "text/plain", "Forbidden");
    }
    if mutation && !permit {
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
fn response(status: u16, mime: &str, body: &str) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        429 => "Too Many Requests",
        _ => "Service Unavailable",
    };
    format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'\r\n\r\n{body}", body.len()).into_bytes()
}

fn status_json(status_snapshot: &StatusSnapshot) -> String {
    // Hand-rolled to dodge serde codegen on a hot path.
    format!(
        "{{\"playing\":{},\"title\":\"{}\",\"artist\":\"{}\",\"position\":{:.3},\"duration\":{:.3},\"volume\":{:.1}}}",
        status_snapshot.playing,
        escape_json(&status_snapshot.title.chars().take(2048).collect::<String>()),
        escape_json(
            &status_snapshot
                .artist
                .chars()
                .take(2048)
                .collect::<String>()
        ),
        finite(status_snapshot.position_s),
        finite(status_snapshot.duration_s),
        finite(status_snapshot.volume).min(100.0),
    )
}

fn escape_json(raw_text: &str) -> String {
    let mut escaped_text = String::with_capacity(raw_text.len());
    for character in raw_text.chars() {
        match character {
            '"' => escaped_text.push_str("\\\""),
            '\\' => escaped_text.push_str("\\\\"),
            '\n' => escaped_text.push_str("\\n"),
            '\r' => escaped_text.push_str("\\r"),
            '\t' => escaped_text.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                escaped_text.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped_text.push(character),
        }
    }
    escaped_text
}

fn finite(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
fn parse_volume(query: &str) -> Option<f64> {
    let mut volumes = query.split('&').filter_map(|pair| pair.strip_prefix("v="));
    let value: f64 = volumes.next()?.parse().ok()?;
    if volumes.next().is_some() || !value.is_finite() {
        return None;
    }
    Some(value.clamp(0.0, 100.0))
}
fn token_from_reader(mut reader: impl Read) -> std::io::Result<String> {
    let mut bytes = [0_u8; 16];
    reader.read_exact(&mut bytes)?;
    Ok(hex(&bytes))
}
fn make_token() -> std::io::Result<String> {
    token_from_reader(std::fs::File::open("/dev/urandom")?)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex_text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex_text.push(DIGITS[(byte >> 4) as usize] as char);
        hex_text.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    hex_text
}

const INDEX_HTML: &str = include_str!("remote_control.html");
const INDEX_CSS: &str = include_str!("remote_control.css");
const INDEX_JS: &str = include_str!("remote_control.js");

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    /// Shared with `slow_peer_tests`, which asserts the same shutdown contract.
    pub(super) fn is_closed(result: std::io::Result<usize>) -> bool {
        match result {
            Ok(0) => true,
            Err(error) => matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::UnexpectedEof
            ),
            _ => false,
        }
    }

    #[test]
    fn volume_parse_clamps() {
        assert_eq!(parse_volume("v=50"), Some(50.0));
        assert_eq!(parse_volume("v=150"), Some(100.0));
        assert_eq!(parse_volume("v=-5"), Some(0.0));
        assert_eq!(parse_volume("foo=bar&v=42.5"), Some(42.5));
        assert_eq!(parse_volume("nope"), None);
    }

    #[test]
    fn escape_json_specials() {
        assert_eq!(escape_json("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        assert_eq!(escape_json("a\rb\t\u{1f}"), "a\\rb\\t\\u001f");
        assert_eq!(escape_json("a b"), "a b");
    }

    #[test]
    fn status_json_is_valid_and_escaped() {
        let status_snapshot = StatusSnapshot {
            playing: true,
            title: "A \"quoted\" title\n".into(),
            artist: "Artist\\Name".into(),
            position_s: 12.34567,
            duration_s: 98.76543,
            volume: 42.26,
        };

        let value: serde_json::Value =
            serde_json::from_str(&status_json(&status_snapshot)).unwrap();

        assert_eq!(value["playing"], true);
        assert_eq!(value["title"], "A \"quoted\" title\n");
        assert_eq!(value["artist"], "Artist\\Name");
        assert_eq!(value["position"], 12.346);
        assert_eq!(value["duration"], 98.765);
        assert_eq!(value["volume"], 42.3);
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa2, 0xff]), "000fa2ff");
    }

    #[test]
    fn token_is_32_hex_chars() {
        let t = make_token().unwrap();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(t.chars().all(|c| !c.is_ascii_uppercase()));
    }

    #[test]
    fn token_failure_is_closed_and_routes_have_an_exact_boundary() {
        assert!(token_from_reader(std::io::Cursor::new([])).is_err());
        assert!(route("/secret-extra/status", "secret").is_none());
        assert_eq!(route("/secret/status", "secret"), Some("status"));
        assert_eq!(parse_volume("v=NaN"), None);
        assert_eq!(parse_volume("v=2&v=3"), None);
    }

    #[test]
    fn server_handle_stop_sets_shutdown_flag() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let handle = ServerHandle {
            port: 12345,
            token: "token".into(),
            stop: Arc::clone(&stop_flag),
            wake: None,
        };

        handle.stop();

        assert!(stop_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn server_handle_drop_sets_shutdown_flag() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        {
            let _handle = ServerHandle {
                port: 12345,
                token: "token".into(),
                stop: Arc::clone(&stop_flag),
                wake: None,
            };
        }

        assert!(stop_flag.load(Ordering::Relaxed));
    }

    fn send_http_request(port: u16, method: &str, request_target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let request = format!(
            "{method} {request_target} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Big-Media-Remote: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).unwrap();

        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn response_body(response: &str) -> &str {
        response.split_once("\r\n\r\n").unwrap().1
    }

    fn assert_ok_response(response: &str) {
        assert!(response.starts_with("HTTP/1.1 200"));
        assert_eq!(response_body(response), "ok");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn local_http_routes_dispatch_handlers_and_render_responses() {
        // This is a live loopback TCP integration test. Miri can execute the
        // pure route helpers above, but socket readiness is outside its UB model.
        let play_pause_count = Arc::new(AtomicUsize::new(0));
        let next_count = Arc::new(AtomicUsize::new(0));
        let previous_count = Arc::new(AtomicUsize::new(0));
        let status_count = Arc::new(AtomicUsize::new(0));
        let observed_volumes = Arc::new(Mutex::new(Vec::new()));

        let handlers = RemoteHandlers {
            play_pause: {
                let play_pause_count = Arc::clone(&play_pause_count);
                Arc::new(move || {
                    play_pause_count.fetch_add(1, Ordering::Relaxed);
                    true
                })
            },
            next: {
                let next_count = Arc::clone(&next_count);
                Arc::new(move || {
                    next_count.fetch_add(1, Ordering::Relaxed);
                    true
                })
            },
            prev: {
                let previous_count = Arc::clone(&previous_count);
                Arc::new(move || {
                    previous_count.fetch_add(1, Ordering::Relaxed);
                    true
                })
            },
            set_volume: {
                let observed_volumes = Arc::clone(&observed_volumes);
                Arc::new(move |volume| {
                    observed_volumes.lock().unwrap().push(volume);
                    true
                })
            },
            status: {
                let status_count = Arc::clone(&status_count);
                Arc::new(move || {
                    status_count.fetch_add(1, Ordering::Relaxed);
                    StatusSnapshot {
                        playing: true,
                        title: "Remote \"Track\"".into(),
                        artist: "BigLinux".into(),
                        position_s: 1.25,
                        duration_s: 5.0,
                        volume: 75.0,
                    }
                })
            },
        };

        let server_handle =
            start_scoped("127.0.0.1:0".parse().unwrap(), handlers, Arc::default()).unwrap();
        let token_path = format!("/{}", server_handle.token);

        let index_response =
            send_http_request(server_handle.port, "GET", &format!("{token_path}/"));
        assert!(index_response.starts_with("HTTP/1.1 200"));
        assert!(index_response.contains("Content-Type: text/html"));

        let status_response =
            send_http_request(server_handle.port, "GET", &format!("{token_path}/status"));
        assert!(status_response.starts_with("HTTP/1.1 200"));
        assert!(status_response.contains("Content-Type: application/json"));
        let status_value: serde_json::Value =
            serde_json::from_str(response_body(&status_response)).unwrap();
        assert_eq!(status_value["playing"], true);
        assert_eq!(status_value["title"], "Remote \"Track\"");
        assert_eq!(status_count.load(Ordering::Relaxed), 1);

        let denied = send_http_request(
            server_handle.port,
            "GET",
            &format!("{token_path}/play-pause"),
        );
        assert!(denied.starts_with("HTTP/1.1 405"));
        assert_eq!(play_pause_count.load(Ordering::Relaxed), 0);
        assert_ok_response(&send_http_request(
            server_handle.port,
            "POST",
            &format!("{token_path}/play-pause"),
        ));
        assert_ok_response(&send_http_request(
            server_handle.port,
            "POST",
            &format!("{token_path}/next"),
        ));
        assert_ok_response(&send_http_request(
            server_handle.port,
            "POST",
            &format!("{token_path}/prev"),
        ));
        assert_ok_response(&send_http_request(
            server_handle.port,
            "POST",
            &format!("{token_path}/volume?v=150"),
        ));

        assert_eq!(play_pause_count.load(Ordering::Relaxed), 1);
        assert_eq!(next_count.load(Ordering::Relaxed), 1);
        assert_eq!(previous_count.load(Ordering::Relaxed), 1);
        assert_eq!(*observed_volumes.lock().unwrap(), vec![100.0]);

        let unauthorized_response =
            send_http_request(server_handle.port, "GET", "/wrong-token/status");
        assert!(unauthorized_response.starts_with("HTTP/1.1 404"));
        assert!(unauthorized_response.contains("Not Found"));
        assert_eq!(status_count.load(Ordering::Relaxed), 1);

        server_handle.stop();
    }
}

#[cfg(test)]
mod slow_peer_tests {
    use super::tests::is_closed;
    use super::*;
    fn handlers(accepted: bool) -> RemoteHandlers {
        RemoteHandlers {
            play_pause: Arc::new(move || accepted),
            next: Arc::new(move || accepted),
            prev: Arc::new(move || accepted),
            set_volume: Arc::new(move |_| accepted),
            status: Arc::new(|| StatusSnapshot {
                playing: false,
                title: String::new(),
                artist: String::new(),
                position_s: 0.0,
                duration_s: 0.0,
                volume: 0.0,
            }),
        }
    }
    #[test]
    fn parser_rejects_smuggling_and_duplicate_headers() {
        for input in [
            "POST /token/next HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1",
            "POST /token/next HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nContent-Length: 0",
            "POST /token/next HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked",
            "GET /token/status HTTP/1.1\r\nHost: localhost\r\n folded:value",
            "GET /token/status HTTP/1.1\r\nHost: localhost\r\nExpect: 100-continue",
        ] {
            assert!(parse_request(input.as_bytes()).is_err(), "{input}");
        }
    }
    #[test]
    fn rate_limit_is_bounded_and_resets_by_window_not_each_byte() {
        let now = Instant::now();
        let mut rate = Rate {
            start: now,
            count: 0,
        };
        for _ in 0..8 {
            assert!(rate.allow(now, 8));
        }
        assert!(!rate.allow(now, 8));
        assert!(rate.allow(now + Duration::from_secs(1), 8));
    }
    #[test]
    fn denied_gui_admission_is_not_a_success_response() {
        let req =
            parse_request(b"POST /token/next HTTP/1.1\r\nHost: localhost\r\nX-Big-Media-Remote: 1")
                .unwrap();
        let reply = dispatch(
            &req,
            "token",
            &handlers(false),
            &AtomicBool::new(false),
            true,
        );
        assert!(reply.starts_with(b"HTTP/1.1 503"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn absolute_header_deadline_is_not_extended_by_activity() {
        let server = start_scoped(
            "127.0.0.1:0".parse().unwrap(),
            handlers(true),
            Arc::default(),
        )
        .unwrap();
        let mut peer = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        peer.write_all(b"GET /").unwrap();
        thread::sleep(HEADER_DEADLINE + Duration::from_millis(150));
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut byte = [0];
        assert!(is_closed(peer.read(&mut byte)));
        server.stop();
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn slow_peers_do_not_block_status_and_stop_releases_sockets() {
        let server = start_scoped(
            "127.0.0.1:0".parse().unwrap(),
            handlers(true),
            Arc::default(),
        )
        .unwrap();
        let mut slow: Vec<_> = (0..MAX_PER_IP - 1)
            .map(|_| TcpStream::connect(("127.0.0.1", server.port)).unwrap())
            .collect();
        for stream in &mut slow {
            stream.write_all(b"GET /").unwrap();
        }
        let mut fast = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        fast.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        fast.write_all(
            format!(
                "GET /{}/status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                server.token
            )
            .as_bytes(),
        )
        .unwrap();
        let mut response = Vec::new();
        fast.read_to_end(&mut response).unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200"));
        server.stop();
        for mut stream in slow {
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut byte = [0];
            assert!(is_closed(stream.read(&mut byte)));
        }
    }
}
