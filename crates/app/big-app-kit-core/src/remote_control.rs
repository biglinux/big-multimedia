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
//! Requests outside `/<token>/...` are answered with 404. The token is
//! per-session and shared with the phone only through the QR code, so LAN
//! neighbors cannot guess the URL.

use log::info;
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
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
    wake: SocketAddr,
}
impl ServerHandle {
    /// Revoke new dispatch immediately; drain no network uploads or writes.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        // An idle worker blocks in accept; a loopback connect returns it to the stop check.
        let _ = TcpStream::connect_timeout(&self.wake, Duration::from_millis(250));
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
const MUTATIONS: [&str; 4] = ["play-pause", "next", "prev", "volume"];

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
    let local = listener.local_addr().map_err(|e| format!("address: {e}"))?;
    let wake = match local.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::new(Ipv4Addr::LOCALHOST.into(), local.port())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::new(Ipv6Addr::LOCALHOST.into(), local.port())
        }
        _ => local,
    };
    let flag = stop.clone();
    let secret = token.clone();
    // Detached: Drop on the GTK thread must not join.
    thread::Builder::new()
        .name("media-remote".into())
        .spawn(move || serve(&listener, &secret, &handlers, &flag))
        .map_err(|_| "Could not start remote control worker".to_owned())?;
    info!("web-remote listening on port {}", local.port());
    Ok(ServerHandle {
        port: local.port(),
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
impl Client {
    /// Reads what the peer sent so far. `Ok(true)` once the head is complete.
    fn read_head(&mut self, now: Instant) -> Result<bool, ()> {
        // Absolute deadline from accept, not reset by one byte per second.
        if now.duration_since(self.accepted) >= HEADER_DEADLINE {
            return Err(());
        }
        let mut chunk = [0u8; 2048];
        match self.stream.read(&mut chunk) {
            Ok(0) => Err(()),
            Ok(n) if self.input.len() + n > MAX_HEADER => Err(()),
            Ok(n) => {
                self.input.extend_from_slice(&chunk[..n]);
                Ok(self.input.windows(4).any(|w| w == b"\r\n\r\n"))
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                Ok(false)
            }
            Err(_) => Err(()),
        }
    }

    /// Writes the next slice of the response. False once the peer is done or stuck.
    fn write_output(&mut self, now: Instant) -> bool {
        let Some(output) = &self.output else {
            return true;
        };
        if self
            .write_started
            .is_some_and(|start| now.duration_since(start) >= WRITE_DEADLINE)
        {
            return false;
        }
        let end = (self.sent + 8192).min(output.len());
        match self.stream.write(&output[self.sent..end]) {
            Ok(0) => false,
            Ok(n) => {
                self.sent += n;
                self.sent < output.len()
            }
            Err(e) => matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted),
        }
    }
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

/// Mutation budgets: 8 per second per peer, 24 per second overall.
struct Limits {
    peers: HashMap<IpAddr, Rate>,
    global: Rate,
}
impl Limits {
    fn admit(&mut self, ip: IpAddr, now: Instant) -> bool {
        if !self.peers.contains_key(&ip) && self.peers.len() >= MAX_RATE_PEERS {
            return false;
        }
        let peer = self.peers.entry(ip).or_insert(Rate {
            start: now,
            count: 0,
        });
        peer.allow(now, 8) && self.global.allow(now, 24)
    }
}

enum Accepted {
    More,
    Drained,
    Failed,
}
fn admit_client(
    result: std::io::Result<(TcpStream, SocketAddr)>,
    clients: &mut Vec<Client>,
) -> Accepted {
    let (stream, peer) = match result {
        Ok(accepted) => accepted,
        Err(e) if e.kind() == ErrorKind::WouldBlock => return Accepted::Drained,
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::Interrupted | ErrorKind::ConnectionAborted
            ) =>
        {
            return Accepted::More;
        }
        Err(_) => return Accepted::Failed,
    };
    let full = clients.len() >= MAX_CLIENTS
        || clients.iter().filter(|c| c.ip == peer.ip()).count() >= MAX_PER_IP;
    if full || stream.set_nonblocking(true).is_err() {
        let _ = stream.shutdown(Shutdown::Both);
        return Accepted::More;
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
    Accepted::More
}

/// Waits for a connection without waking while nothing happens.
fn accept_blocking(listener: &TcpListener) -> std::io::Result<(TcpStream, SocketAddr)> {
    listener.set_nonblocking(false)?;
    let accepted = listener.accept();
    listener.set_nonblocking(true)?;
    accepted
}

fn serve(listener: &TcpListener, token: &str, handlers: &RemoteHandlers, stop: &AtomicBool) {
    let mut clients: Vec<Client> = Vec::with_capacity(MAX_CLIENTS);
    let mut limits = Limits {
        peers: HashMap::new(),
        global: Rate {
            start: Instant::now(),
            count: 0,
        },
    };
    while !stop.load(Ordering::Acquire) {
        let mut next = if clients.is_empty() {
            accept_blocking(listener)
        } else {
            listener.accept()
        };
        // Finite accept budget per iteration; floods cannot starve existing peers.
        for _ in 0..MAX_CLIENTS {
            match admit_client(next, &mut clients) {
                Accepted::More => next = listener.accept(),
                Accepted::Drained => break,
                Accepted::Failed => {
                    stop.store(true, Ordering::Release);
                    break;
                }
            }
        }
        let now = Instant::now();
        limits
            .peers
            .retain(|_, rate| now.duration_since(rate.start) < Duration::from_secs(10));
        clients.retain_mut(|client| {
            if stop.load(Ordering::Acquire) {
                return false;
            }
            if client.output.is_none() {
                match client.read_head(now) {
                    Err(()) => return false,
                    Ok(false) => return true,
                    Ok(true) => {
                        let ip = client.ip;
                        let admit = || limits.admit(ip, now);
                        client.output = Some(respond(&client.input, token, handlers, stop, admit));
                        client.write_started = Some(now);
                        client.input.clear();
                    }
                }
            }
            client.write_output(now)
        });
        if !clients.is_empty() {
            thread::sleep(Duration::from_millis(10));
        }
    }
    for client in clients {
        let _ = client.stream.shutdown(Shutdown::Both);
    }
    info!("web-remote stopped");
}

/// Answers one complete request head. `admit` charges the mutation rate budget.
fn respond(
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
    use std::sync::Mutex;

    fn is_closed(result: std::io::Result<usize>) -> bool {
        match result {
            Ok(0) => true,
            Err(error) => matches!(
                error.kind(),
                ErrorKind::ConnectionReset
                    | ErrorKind::BrokenPipe
                    | ErrorKind::NotConnected
                    | ErrorKind::UnexpectedEof
            ),
            _ => false,
        }
    }

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

    fn start(handlers: RemoteHandlers) -> ServerHandle {
        start_scoped("127.0.0.1:0".parse().unwrap(), handlers, Arc::default()).unwrap()
    }

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
    fn token_is_32_lowercase_hex() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa2, 0xff]), "000fa2ff");
        let token = make_token().unwrap();
        assert_eq!(token.len(), 32);
        assert!(
            token
                .bytes()
                .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
        );
        assert!(token_from_reader(std::io::Cursor::new([])).is_err());
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
            "GET /token/status HTTP/1.1\r\nHost: localhost\r\nExpect: 100-continue",
        ] {
            assert!(parse_request(input.as_bytes()).is_err(), "{input}");
        }
    }

    #[test]
    fn rate_limit_resets_per_window() {
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
    fn denied_admission_returns_503() {
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

    fn send_http_request(port: u16, method: &str, request_target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let request = format!(
            "{method} {request_target} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Big-Media-Remote: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    #[cfg_attr(miri, ignore)] // live loopback sockets
    fn http_routes_dispatch_to_handlers() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let record = |name: &'static str| {
            let calls = Arc::clone(&calls);
            Arc::new(move || {
                calls.lock().unwrap().push(name.to_owned());
                true
            })
        };
        let server = start(RemoteHandlers {
            play_pause: record("play-pause"),
            next: record("next"),
            prev: record("prev"),
            set_volume: {
                let calls = Arc::clone(&calls);
                Arc::new(move |volume| {
                    calls.lock().unwrap().push(format!("volume {volume}"));
                    true
                })
            },
            status: Arc::new(|| StatusSnapshot {
                playing: true,
                title: "Remote \"Track\"".into(),
                artist: "BigLinux".into(),
                position_s: 1.25,
                duration_s: 5.0,
                volume: 75.0,
            }),
        });
        let send = |method, path: &str| {
            send_http_request(server.port, method, &format!("/{}/{path}", server.token))
        };

        let index = send("GET", "");
        assert!(index.starts_with("HTTP/1.1 200") && index.contains("Content-Type: text/html"));
        let status = send("GET", "status");
        let (_, body) = status.split_once("\r\n\r\n").unwrap();
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(value["title"], "Remote \"Track\"");
        assert!(send("GET", "play-pause").starts_with("HTTP/1.1 405"));
        for path in ["play-pause", "next", "prev", "volume?v=150"] {
            assert!(send("POST", path).ends_with("\r\n\r\nok"), "{path}");
        }
        assert_eq!(
            *calls.lock().unwrap(),
            ["play-pause", "next", "prev", "volume 100"]
        );
        let unauthorized = send_http_request(server.port, "GET", "/wrong-token/status");
        assert!(unauthorized.starts_with("HTTP/1.1 404"));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn stop_ends_idle_worker() {
        let worker_alive = Arc::new(());
        let mut handlers = handlers(true);
        let held = Arc::clone(&worker_alive);
        handlers.next = Arc::new(move || {
            let _ = &held;
            true
        });
        let server = start(handlers);
        thread::sleep(Duration::from_millis(50)); // let the worker block in accept
        drop(server);
        let deadline = Instant::now() + Duration::from_secs(1);
        while Arc::strong_count(&worker_alive) > 1 {
            assert!(Instant::now() < deadline, "worker still running");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn header_deadline_ignores_activity() {
        let server = start(handlers(true));
        let mut peer = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        peer.write_all(b"GET /").unwrap();
        thread::sleep(HEADER_DEADLINE + Duration::from_millis(150));
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut byte = [0];
        assert!(is_closed(peer.read(&mut byte)));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn slow_peers_leave_server_responsive() {
        let server = start(handlers(true));
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
