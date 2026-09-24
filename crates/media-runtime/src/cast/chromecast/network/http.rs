use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use log::{info, warn};

use super::guess_mime;
use crate::error::{Error, Result};

/// ~10 s ahead of Chromecast consumption at typical bitrates.
const MAX_BUFFER_AHEAD: usize = 10 * 1024 * 1024;
/// Trim consumed bytes when read_pos crosses this threshold.
const TRIM_THRESHOLD: usize = 4 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HttpMethod {
    Get,
    Head,
    Unsupported,
}

impl HttpMethod {
    fn allows_body(self) -> bool {
        matches!(self, Self::Get)
    }
}

#[derive(Debug)]
struct HttpRequest {
    method: HttpMethod,
    target: String,
    range_header: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ByteRange {
    start: u64,
    end: u64,
}

impl ByteRange {
    fn len(self) -> u64 {
        self.end - self.start + 1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ByteRangeError {
    BadSyntax,
    Unsatisfiable,
}

struct PipeBuffer {
    data: Vec<u8>,
    read_pos: usize,
    /// ffmpeg's output ended (EOF or read error).
    done: bool,
}

/// ffmpeg output waiting for the Chromecast. `changed` wakes the side that
/// waits: the HTTP reader for new bytes, the ffmpeg reader for free space.
struct Pipe {
    buffer: Mutex<PipeBuffer>,
    changed: Condvar,
}

/// Longest a waiter sleeps before rechecking the session's stop flag, which is
/// set from outside and does not signal `changed`.
const STOP_RECHECK: Duration = Duration::from_millis(200);

impl Pipe {
    fn lock(&self) -> MutexGuard<'_, PipeBuffer> {
        self.buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wait<'a>(&self, buffer: MutexGuard<'a, PipeBuffer>) -> MutexGuard<'a, PipeBuffer> {
        self.changed
            .wait_timeout(buffer, STOP_RECHECK)
            .map_or_else(|poisoned| poisoned.into_inner().0, |(buffer, _)| buffer)
    }
}

struct PipeBufferReader {
    pipe: Arc<Pipe>,
    stop_flag: Arc<AtomicBool>,
}

impl Read for PipeBufferReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut state = self.pipe.lock();
        loop {
            if self.stop_flag.load(Ordering::Relaxed) {
                return Ok(0);
            }
            let available = state.data.len().saturating_sub(state.read_pos);
            if available > 0 {
                let to_read = available.min(buf.len());
                buf[..to_read]
                    .copy_from_slice(&state.data[state.read_pos..state.read_pos + to_read]);
                state.read_pos += to_read;
                if state.read_pos > TRIM_THRESHOLD {
                    let pos = state.read_pos;
                    state.data.drain(..pos);
                    state.read_pos = 0;
                }
                drop(state);
                // Space freed: let the ffmpeg side resume if it was held back.
                self.pipe.changed.notify_all();
                return Ok(to_read);
            }
            if state.done {
                return Ok(0);
            }
            state = self.pipe.wait(state);
        }
    }
}

/// 128-bit hex token gating HTTP requests. The server binds 0.0.0.0 so the
/// Chromecast can reach it, but LAN neighbors hitting any other path get 404.
/// Without kernel randomness there is no safe token, so the server refuses to
/// start rather than expose the file behind a guessable one.
fn make_session_token() -> Result<String> {
    let mut bytes = [0_u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| Error::chromecast(format!("no randomness for the stream token: {e}")))?;
    Ok(encode_hex(&bytes))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Option<HttpRequest>> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(None);
    }

    let mut total_header_bytes = request_line.len();
    if total_header_bytes > MAX_HEADER_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "HTTP request header too large",
        ));
    }

    let mut parts = request_line.split_whitespace();
    let method = match parts.next() {
        Some("GET") => HttpMethod::Get,
        Some("HEAD") => HttpMethod::Head,
        Some(_) | None => HttpMethod::Unsupported,
    };
    let target = parts.next().unwrap_or_default().to_string();

    let mut range_header = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        total_header_bytes += read;
        if total_header_bytes > MAX_HEADER_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP request header too large",
            ));
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("Range")
        {
            range_header = Some(value.trim().to_string());
        }
    }

    Ok(Some(HttpRequest {
        method,
        target,
        range_header,
    }))
}

fn request_path(target: &str) -> &str {
    if let Some(rest) = target.strip_prefix("http://") {
        return rest.find('/').map_or("/", |path_start| &rest[path_start..]);
    }
    if let Some(rest) = target.strip_prefix("https://") {
        return rest.find('/').map_or("/", |path_start| &rest[path_start..]);
    }
    target
}

fn is_authorized_target(target: &str, token: &str) -> bool {
    let path = request_path(target);
    let path = path
        .split_once('?')
        .map_or(path, |(path_only, _)| path_only);
    // Constant-time: the server is reachable from the whole LAN.
    path.strip_prefix('/').is_some_and(|candidate| {
        candidate.len() == token.len()
            && candidate
                .bytes()
                .zip(token.bytes())
                .fold(0, |diff, (a, b)| diff | (a ^ b))
                == 0
    })
}

fn parse_byte_range(
    header_value: &str,
    total_len: u64,
) -> std::result::Result<ByteRange, ByteRangeError> {
    if total_len == 0 {
        return Err(ByteRangeError::Unsatisfiable);
    }

    let spec = header_value
        .trim()
        .strip_prefix("bytes=")
        .ok_or(ByteRangeError::BadSyntax)?;
    if spec.contains(',') {
        return Err(ByteRangeError::BadSyntax);
    }

    let (start_text, end_text) = spec.split_once('-').ok_or(ByteRangeError::BadSyntax)?;
    let last_byte = total_len - 1;

    if start_text.is_empty() {
        let suffix_len = end_text
            .parse::<u64>()
            .map_err(|_| ByteRangeError::BadSyntax)?;
        if suffix_len == 0 {
            return Err(ByteRangeError::Unsatisfiable);
        }
        let start = total_len.saturating_sub(suffix_len);
        return Ok(ByteRange {
            start,
            end: last_byte,
        });
    }

    let start = start_text
        .parse::<u64>()
        .map_err(|_| ByteRangeError::BadSyntax)?;
    let end = if end_text.is_empty() {
        last_byte
    } else {
        end_text
            .parse::<u64>()
            .map_err(|_| ByteRangeError::BadSyntax)?
            .min(last_byte)
    };

    if start > end || start > last_byte {
        return Err(ByteRangeError::Unsatisfiable);
    }

    Ok(ByteRange { start, end })
}

fn write_headers(
    stream: &mut TcpStream,
    status_code: u16,
    reason: &str,
    headers: &[(&str, String)],
) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status_code} {reason}\r\n")?;
    for (name, value) in headers {
        let value = value.replace(['\r', '\n'], "");
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "Connection: close\r\n\r\n")
}

fn write_text_response(
    stream: &mut TcpStream,
    status_code: u16,
    reason: &str,
    body: &str,
) -> std::io::Result<()> {
    let headers = [
        ("Content-Type", "text/plain; charset=utf-8".to_string()),
        ("Content-Length", body.len().to_string()),
    ];
    write_headers(stream, status_code, reason, &headers)?;
    stream.write_all(body.as_bytes())
}

fn write_file_response(
    stream: &mut TcpStream,
    mut file: std::fs::File,
    mime: &str,
    total_len: u64,
    range: Option<ByteRange>,
    should_write_body: bool,
) -> std::io::Result<()> {
    if let Some(range) = range {
        file.seek(std::io::SeekFrom::Start(range.start))?;
        let headers = [
            ("Content-Type", mime.to_string()),
            (
                "Content-Range",
                format!("bytes {}-{}/{}", range.start, range.end, total_len),
            ),
            ("Accept-Ranges", "bytes".to_string()),
            ("Content-Length", range.len().to_string()),
        ];
        write_headers(stream, 206, "Partial Content", &headers)?;
        if should_write_body {
            let mut limited = file.take(range.len());
            std::io::copy(&mut limited, stream)?;
        }
    } else {
        let headers = [
            ("Content-Type", mime.to_string()),
            ("Accept-Ranges", "bytes".to_string()),
            ("Content-Length", total_len.to_string()),
        ];
        write_headers(stream, 200, "OK", &headers)?;
        if should_write_body {
            std::io::copy(&mut file, stream)?;
        }
    }

    Ok(())
}

fn prepare_http_stream(stream: &mut TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
}

fn accept_next_connection(listener: &TcpListener, stop_flag: &AtomicBool) -> Option<TcpStream> {
    loop {
        if stop_flag.load(Ordering::Relaxed) {
            return None;
        }
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                warn!("HTTP accept error: {e}");
                return None;
            }
        }
    }
}

fn handle_file_request(mut stream: TcpStream, token: &str, path: &str, mime: &str) {
    prepare_http_stream(&mut stream);

    let request = match read_http_request(&mut stream) {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(e) => {
            warn!("HTTP request parse error: {e}");
            let _ = write_text_response(&mut stream, 400, "Bad Request", "Bad Request");
            return;
        }
    };

    if request.method == HttpMethod::Unsupported {
        let _ = write_text_response(&mut stream, 405, "Method Not Allowed", "Method Not Allowed");
        return;
    }

    if !is_authorized_target(&request.target, token) {
        let _ = write_text_response(&mut stream, 404, "Not Found", "Not Found");
        return;
    }

    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) => {
            warn!("HTTP: failed to open file: {e}");
            let _ = write_text_response(&mut stream, 404, "Not Found", "Not Found");
            return;
        }
    };
    let total_len = file.metadata().map_or(0, |metadata| metadata.len());

    let range = match request.range_header.as_deref() {
        Some(range_header) => match parse_byte_range(range_header, total_len) {
            Ok(range) => Some(range),
            Err(ByteRangeError::BadSyntax) => {
                let _ = write_text_response(&mut stream, 400, "Bad Range", "Bad Range");
                return;
            }
            Err(ByteRangeError::Unsatisfiable) => {
                let _ = write_text_response(
                    &mut stream,
                    416,
                    "Range Not Satisfiable",
                    "Range Not Satisfiable",
                );
                return;
            }
        },
        None => None,
    };

    if let Err(e) = write_file_response(
        &mut stream,
        file,
        mime,
        total_len,
        range,
        request.method.allows_body(),
    ) {
        warn!("HTTP file response error: {e}");
    }
}

fn handle_pipe_request(
    mut stream: TcpStream,
    token: &str,
    pipe: &Arc<Pipe>,
    stop_flag: &Arc<AtomicBool>,
) {
    prepare_http_stream(&mut stream);

    let request = match read_http_request(&mut stream) {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(e) => {
            warn!("HTTP pipe request parse error: {e}");
            let _ = write_text_response(&mut stream, 400, "Bad Request", "Bad Request");
            return;
        }
    };

    if request.method == HttpMethod::Unsupported {
        let _ = write_text_response(&mut stream, 405, "Method Not Allowed", "Method Not Allowed");
        return;
    }

    if !is_authorized_target(&request.target, token) {
        let _ = write_text_response(&mut stream, 404, "Not Found", "Not Found");
        return;
    }

    {
        // Give ffmpeg up to 4 s to produce the first bytes before answering.
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut buffer = pipe.lock();
        while buffer.data.is_empty() && !buffer.done && Instant::now() < deadline {
            buffer = pipe.wait(buffer);
        }
    }

    let headers = [("Content-Type", "video/mp4".to_string())];
    if let Err(e) = write_headers(&mut stream, 200, "OK", &headers) {
        warn!("HTTP pipe header error: {e}");
        return;
    }

    if request.method.allows_body() {
        let mut pipe_reader = PipeBufferReader {
            pipe: Arc::clone(pipe),
            stop_flag: Arc::clone(stop_flag),
        };
        if let Err(e) = std::io::copy(&mut pipe_reader, &mut stream) {
            warn!("HTTP pipe response error: {e}");
        }
    }
}

/// Returns `(port, token)` - caller builds `http://<local_ip>:<port>/<token>`.
/// Any other path -> 404, blocking LAN-neighbor URL guessing.
pub(in crate::cast::chromecast) fn start_file_server(
    file_path: &str,
    stop_flag: Arc<AtomicBool>,
) -> Result<(u16, String)> {
    let listener = TcpListener::bind("0.0.0.0:0")
        .map_err(|e| Error::chromecast(format!("bind failed: {e}")))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| Error::chromecast(format!("nonblocking listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| Error::chromecast(format!("local addr: {e}")))?
        .port();

    let token = make_session_token()?;
    let token_for_thread = token.clone();
    let path = file_path.to_string();
    thread::spawn(move || {
        let mime = guess_mime(&path);
        // One thread per connection: the Chromecast opens a second connection
        // (a HEAD probe, or a fresh ranged GET on seek/reconnect) while a long
        // GET is still streaming; serial handling would stall it behind the
        // in-flight transfer. Each request opens its own File handle, so the
        // handlers share no per-connection state.
        while let Some(stream) = accept_next_connection(&listener, &stop_flag) {
            let token = token_for_thread.clone();
            let path = path.clone();
            let mime = mime.clone();
            thread::spawn(move || handle_file_request(stream, &token, &path, &mime));
        }
        info!("HTTP file server stopped on port {port}");
    });

    info!("HTTP file server started on port {port}");
    Ok((port, token))
}

/// Streams an FFmpeg pipe (ChildStdout). Same `(port, token)` contract as
/// `start_file_server` - non-token paths get 404.
pub(in crate::cast::chromecast) fn start_pipe_server(
    stdout: std::process::ChildStdout,
    stop_flag: Arc<AtomicBool>,
) -> Result<(u16, String)> {
    let listener = TcpListener::bind("0.0.0.0:0")
        .map_err(|e| Error::chromecast(format!("bind failed: {e}")))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| Error::chromecast(format!("nonblocking listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| Error::chromecast(format!("local addr: {e}")))?
        .port();
    let token = make_session_token()?;
    let token_for_thread = token.clone();

    let pipe = Arc::new(Pipe {
        buffer: Mutex::new(PipeBuffer {
            data: Vec::with_capacity(1024 * 1024),
            read_pos: 0,
            done: false,
        }),
        changed: Condvar::new(),
    });

    {
        let pipe = Arc::clone(&pipe);
        let stop = Arc::clone(&stop_flag);
        thread::spawn(move || {
            let mut reader = stdout;
            let mut chunk = vec![0_u8; 64 * 1024];
            loop {
                // Hold ffmpeg back while the Chromecast is far behind.
                let mut buffer = pipe.lock();
                while buffer.data.len() - buffer.read_pos > MAX_BUFFER_AHEAD
                    && !stop.load(Ordering::Relaxed)
                {
                    buffer = pipe.wait(buffer);
                }
                drop(buffer);
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        pipe.lock().data.extend_from_slice(&chunk[..n]);
                        pipe.changed.notify_all();
                    }
                    Err(e) => {
                        warn!("Pipe reader error: {e}");
                        break;
                    }
                }
            }
            pipe.lock().done = true;
            pipe.changed.notify_all();
            info!("Pipe reader finished");
        });
    }

    thread::spawn(move || {
        while let Some(stream) = accept_next_connection(&listener, &stop_flag) {
            handle_pipe_request(stream, &token_for_thread, &pipe, &stop_flag);
        }
        info!("HTTP pipe server stopped on port {port}");
    });

    info!("HTTP pipe server started on port {port}");
    Ok((port, token))
}

#[cfg(test)]
mod tests {
    use super::{
        ByteRange, ByteRangeError, MAX_HEADER_BYTES, Pipe, PipeBuffer, PipeBufferReader,
        is_authorized_target, parse_byte_range,
    };
    use std::io::Read;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Condvar, Mutex};

    #[test]
    fn pipe_reader_wakes_on_data_and_ends_on_done() {
        let pipe = Arc::new(Pipe {
            buffer: Mutex::new(PipeBuffer {
                data: Vec::new(),
                read_pos: 0,
                done: false,
            }),
            changed: Condvar::new(),
        });
        let writer = {
            let pipe = Arc::clone(&pipe);
            std::thread::spawn(move || {
                for chunk in [b"abc".as_slice(), b"def"] {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    pipe.lock().data.extend_from_slice(chunk);
                    pipe.changed.notify_all();
                }
                pipe.lock().done = true;
                pipe.changed.notify_all();
            })
        };
        let mut reader = PipeBufferReader {
            pipe,
            stop_flag: Arc::new(AtomicBool::new(false)),
        };
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        writer.join().unwrap();
        assert_eq!(out, b"abcdef");
    }

    #[test]
    fn token_guard_accepts_only_exact_path() {
        assert!(is_authorized_target("/abc123", "abc123"));
        assert!(is_authorized_target("/abc123?x=1", "abc123"));
        assert!(is_authorized_target(
            "http://192.168.1.10:4000/abc123",
            "abc123"
        ));

        assert!(!is_authorized_target("/abc1234", "abc123"));
        assert!(!is_authorized_target("/abc123/extra", "abc123"));
        assert!(!is_authorized_target("/", "abc123"));
    }

    #[test]
    fn byte_ranges_cover_cast_header_patterns() {
        assert_eq!(
            parse_byte_range("bytes=10-19", 100),
            Ok(ByteRange { start: 10, end: 19 })
        );
        assert_eq!(
            parse_byte_range("bytes=10-", 100),
            Ok(ByteRange { start: 10, end: 99 })
        );
        assert_eq!(
            parse_byte_range("bytes=-20", 100),
            Ok(ByteRange { start: 80, end: 99 })
        );
        assert_eq!(
            parse_byte_range("bytes=90-999", 100),
            Ok(ByteRange { start: 90, end: 99 })
        );
    }

    #[test]
    fn byte_ranges_reject_invalid_requests() {
        assert_eq!(
            parse_byte_range("items=0-10", 100),
            Err(ByteRangeError::BadSyntax)
        );
        assert_eq!(
            parse_byte_range("bytes=5-1", 100),
            Err(ByteRangeError::Unsatisfiable)
        );
        assert_eq!(
            parse_byte_range("bytes=0-10", 0),
            Err(ByteRangeError::Unsatisfiable)
        );
        assert_eq!(
            parse_byte_range("bytes=0-1,4-5", 100),
            Err(ByteRangeError::BadSyntax)
        );
    }

    #[test]
    fn header_limit_stays_small() {
        assert_eq!(MAX_HEADER_BYTES, 16 * 1024);
    }
}
