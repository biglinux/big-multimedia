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

use log::{info, warn};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

mod request;
use request::respond;

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
const HEADER_DEADLINE: Duration = Duration::from_secs(3);
const WRITE_DEADLINE: Duration = Duration::from_secs(2);
const MAX_RATE_PEERS: usize = 64;
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

/// The address a phone on the same network reaches this computer at: a private
/// IPv4 address, one on a physical interface first, so a VPN's default route
/// does not decide it. A public or carrier-grade NAT address is never offered.
#[must_use]
pub fn lan_address() -> Option<IpAddr> {
    let addresses = match big_os_kit::interface_addresses::ipv4_interface_addresses() {
        Ok(addresses) => addresses,
        Err(error) => {
            warn!("network interfaces unreadable: {error}");
            return None;
        }
    };
    let candidates = addresses.into_iter().map(|(interface, address)| {
        // Only interfaces backed by a device have one; tunnels and bridges do not.
        let physical = std::path::Path::new("/sys/class/net")
            .join(interface)
            .join("device")
            .exists();
        (address, physical)
    });
    pick_lan_address(candidates).map(IpAddr::V4)
}

fn pick_lan_address(candidates: impl Iterator<Item = (Ipv4Addr, bool)>) -> Option<Ipv4Addr> {
    let private: Vec<(Ipv4Addr, bool)> = candidates
        .filter(|(address, _)| address.is_private())
        .collect();
    private
        .iter()
        .find(|(_, physical)| *physical)
        .or(private.first())
        .map(|(address, _)| *address)
}

/// Start with the same revocation token carried by queued UI commands. `stop`
/// must be fresh and false; a stopped session is never reused for another token.
/// `on_failure` runs on the worker thread when the listener fails for good; it
/// is dropped without a call when the session ends through `stop`.
pub fn start_scoped(
    address: SocketAddr,
    handlers: RemoteHandlers,
    stop: Arc<AtomicBool>,
    on_failure: impl FnOnce(String) + Send + 'static,
) -> Result<ServerHandle, String> {
    if stop.load(Ordering::Acquire) {
        return Err("Remote session already revoked".into());
    }
    let token = big_os_kit::random::random_hex_id()
        .map_err(|_| "Secure randomness unavailable".to_owned())?;
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
        .spawn(move || {
            if let Err(error) = serve(&listener, &secret, &handlers, &flag) {
                warn!("web-remote accept failed: {error}");
                on_failure(error.to_string());
            }
        })
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
    Retry,
    Failed(std::io::Error),
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
        Err(e)
            if matches!(
                e.raw_os_error(),
                Some(libc::EMFILE | libc::ENFILE | libc::ENOBUFS)
            ) =>
        {
            return Accepted::Retry;
        }
        Err(e) => return Accepted::Failed(e),
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

/// Admits pending connections; `Err` once the listener fails for good.
fn accept_burst(listener: &TcpListener, clients: &mut Vec<Client>) -> std::io::Result<()> {
    let mut next = if clients.is_empty() {
        accept_blocking(listener)
    } else {
        listener.accept()
    };
    // Finite accept budget per iteration; floods cannot starve existing peers.
    for _ in 0..MAX_CLIENTS {
        match admit_client(next, clients) {
            Accepted::More => next = listener.accept(),
            Accepted::Drained => break,
            // Exhausted descriptors or buffers free up as peers close.
            Accepted::Retry => {
                thread::sleep(ACCEPT_RETRY_DELAY);
                break;
            }
            Accepted::Failed(error) => return Err(error),
        }
    }
    Ok(())
}

fn serve(
    listener: &TcpListener,
    token: &str,
    handlers: &RemoteHandlers,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    let mut failure = None;
    let mut clients: Vec<Client> = Vec::with_capacity(MAX_CLIENTS);
    let mut limits = Limits {
        peers: HashMap::new(),
        global: Rate {
            start: Instant::now(),
            count: 0,
        },
    };
    while !stop.load(Ordering::Acquire) {
        if let Err(error) = accept_burst(listener, &mut clients) {
            stop.store(true, Ordering::Release);
            failure = Some(error);
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
    failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn lan_address_prefers_private_physical() {
        let tun = (Ipv4Addr::new(10, 8, 0, 2), false);
        let wifi = (Ipv4Addr::new(192, 168, 1, 5), true);
        let bridge = (Ipv4Addr::new(172, 20, 0, 1), false);
        let public = (Ipv4Addr::new(203, 0, 113, 7), true);
        let tailscale = (Ipv4Addr::new(100, 64, 0, 9), false);
        let loopback = (Ipv4Addr::LOCALHOST, false);
        for (candidates, expected) in [
            (vec![loopback, tun, wifi], Some(wifi.0)),
            (vec![public, bridge], Some(bridge.0)),
            (vec![loopback, public, tailscale], None),
            (vec![], None),
        ] {
            assert_eq!(
                pick_lan_address(candidates.clone().into_iter()),
                expected,
                "{candidates:?}"
            );
        }
    }

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

    pub(super) fn handlers(accepted: bool) -> RemoteHandlers {
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
        start_scoped(
            "127.0.0.1:0".parse().unwrap(),
            handlers,
            Arc::default(),
            |_| {},
        )
        .unwrap()
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
    fn exhausted_accept_retries() {
        // errno values from Linux include/uapi/asm-generic/errno-base.h and errno.h.
        let mut clients = Vec::new();
        for errno in [24, 23, 105] {
            let result = Err(std::io::Error::from_raw_os_error(errno));
            assert!(
                matches!(admit_client(result, &mut clients), Accepted::Retry),
                "errno {errno}"
            );
        }
        let invalid = Err(std::io::Error::from_raw_os_error(22));
        assert!(matches!(
            admit_client(invalid, &mut clients),
            Accepted::Failed(_)
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // a live socket
    fn failed_listener_stops_and_reports() {
        // accept on a UDP socket fails with EOPNOTSUPP, which no retry can fix.
        let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let listener = TcpListener::from(std::os::fd::OwnedFd::from(udp));
        let stop = AtomicBool::new(false);
        let result = serve(&listener, "token", &handlers(true), &stop);
        assert_eq!(result.unwrap_err().kind(), ErrorKind::Unsupported);
        assert!(stop.load(Ordering::Acquire));
    }

    pub(super) fn counting_handlers(calls: &Arc<AtomicUsize>) -> RemoteHandlers {
        let command = || {
            let calls = Arc::clone(calls);
            Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                true
            })
        };
        let volume_calls = Arc::clone(calls);
        let status_calls = Arc::clone(calls);
        let mut counting = handlers(true);
        counting.play_pause = command();
        counting.next = command();
        counting.prev = command();
        counting.set_volume = Arc::new(move |_| {
            volume_calls.fetch_add(1, Ordering::SeqCst);
            true
        });
        let status = counting.status;
        counting.status = Arc::new(move || {
            status_calls.fetch_add(1, Ordering::SeqCst);
            status()
        });
        counting
    }

    pub(super) fn status_code(reply: &[u8]) -> &str {
        std::str::from_utf8(&reply[9..12]).unwrap()
    }

    #[test]
    fn rate_limits_return_429() {
        let calls = Arc::new(AtomicUsize::new(0));
        let handlers = counting_handlers(&calls);
        let start = Instant::now();
        let mut limits = Limits {
            peers: HashMap::new(),
            global: Rate { start, count: 0 },
        };
        let mut send = |peer: u8, now: Instant| {
            let reply = respond(
                b"POST /token/next HTTP/1.1\r\nHost: localhost\r\nX-Big-Media-Remote: 1\r\n\r\n",
                "token",
                &handlers,
                &AtomicBool::new(false),
                || limits.admit(IpAddr::from([192, 168, 1, peer]), now),
            );
            status_code(&reply).to_owned()
        };

        // 8 mutations per peer per second.
        for _ in 0..8 {
            assert_eq!(send(1, start), "200");
        }
        assert_eq!(send(1, start), "429");
        // 24 mutations per second overall.
        for peer in [2, 3] {
            for _ in 0..8 {
                assert_eq!(send(peer, start), "200");
            }
        }
        assert_eq!(send(4, start), "429");
        assert_eq!(calls.load(Ordering::SeqCst), 24);
        // At most 64 peers are tracked; one more is refused.
        for peer in 5..=64 {
            let later = start + Duration::from_secs(u64::from(peer));
            assert_eq!(send(peer, later), "200");
        }
        assert_eq!(send(65, start + Duration::from_secs(65)), "429");
        assert_eq!(calls.load(Ordering::SeqCst), 84);
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
