//! Shared HTTP client policy backed by the system `curl` command.
//!
//! The crate keeps the HTTP policy centralized without linking Rust TLS/crypto
//! stacks into every consumer. Sensitive URL and header data goes through
//! curl configuration on stdin, with explicit deadlines and bounded readers.

use std::io::{BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::url::MediaLogTarget;
mod transport;

const CURL: &str = "curl";
const USER_AGENT: &str = "BigLinuxRustApp/0.1 (https://github.com/biglinux/big-multimedia)";
const TIMEOUT_SECS: u64 = 15;
const _: () = assert!(TIMEOUT_SECS >= 5);
const _: () = assert!(TIMEOUT_SECS <= 60);
const FAST_TIMEOUT_SECS: u64 = 5;
const FAST_GLOBAL_TIMEOUT_SECS: u64 = 15;
/// 5 MB cap on text bodies; binary helpers use their own caps.
const MAX_TEXT_BODY_BYTES: u64 = 5 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 128 * 1024;
const DEFAULT_MAX_REDIRECTS: u8 = 10;

/// Response body plus the metadata consumers need without depending on a Rust
/// HTTP crate.
#[derive(Debug, Clone)]
pub struct HttpResponseBytes {
    /// HTTP status code returned by the server after redirects.
    pub status: u16,
    /// Requested URI. The curl backend follows redirects but does not expose
    /// the post-redirect URI yet.
    pub final_url: String,
    /// Response Content-Type header, if present.
    pub content_type: Option<String>,
    /// Response bytes, bounded by the caller-provided cap.
    pub bytes: Vec<u8>,
}

/// Streaming response plus metadata.
pub struct HttpStreamResponse {
    /// HTTP status code returned by the server after redirects.
    pub status: u16,
    /// Requested URI. The curl backend follows redirects but does not expose
    /// the post-redirect URI yet.
    pub final_url: String,
    /// Response Content-Type header, if present.
    pub content_type: Option<String>,
    /// Content-Length header, if present and parseable.
    pub content_length: Option<u64>,
    /// Streaming body reader. Callers must impose their own semantic cap.
    pub reader: Box<dyn Read>,
}

struct CurlHeaders {
    status: u16,
    content_type: Option<String>,
    content_length: Option<u64>,
}

/// DNS/lookup failures are permanent per invocation — never retry.
fn is_dns_error(err: &str) -> bool {
    err.contains("code 6")
        || err.contains("Could not resolve host")
        || err.contains("Name or service not known")
        || err.contains("No address associated")
        || err.contains("failed to lookup address")
        || err.contains("dns error")
}

/// One retry after 500 ms for transient errors (timeout, 5xx); skip on DNS.
pub fn http_get_with_retry(url: &str) -> Option<String> {
    http_get_with_retry_timeout(url, Duration::from_secs(TIMEOUT_SECS))
}

/// Single attempt; caller (radio_api) handles multi-server fallback.
pub fn http_get_fast(url: &str) -> Option<String> {
    http_get_single(url, Duration::from_secs(FAST_GLOBAL_TIMEOUT_SECS))
}

fn http_get_single(url: &str, timeout: Duration) -> Option<String> {
    let url_log = MediaLogTarget(url);
    match curl_bytes(
        url,
        &RequestHeaders::default(),
        &[],
        MAX_TEXT_BODY_BYTES,
        timeout,
        true,
    ) {
        Ok(resp) if resp.status == 200 => String::from_utf8(resp.bytes).ok(),
        Ok(resp) => {
            log::debug!("[http] {url_log}: status={}", resp.status);
            None
        }
        Err(error) => {
            log::debug!("[http] {url_log}: error={error}");
            None
        }
    }
}

fn http_get_with_retry_timeout(url: &str, timeout: Duration) -> Option<String> {
    let url_log = MediaLogTarget(url);
    for attempt in 0..2 {
        match curl_bytes(
            url,
            &RequestHeaders::default(),
            &[],
            MAX_TEXT_BODY_BYTES,
            timeout,
            true,
        ) {
            Ok(resp) if resp.status < 500 => {
                if resp.status != 200 {
                    log::debug!("[http] {url_log}: non-200 status={}", resp.status);
                    return None;
                }
                return String::from_utf8(resp.bytes)
                    .inspect_err(|e| log::debug!("[http] {url_log}: utf8 body error={e}"))
                    .ok();
            }
            Ok(resp) => {
                log::debug!(
                    "[http] {url_log}: 5xx status={} attempt={attempt}",
                    resp.status
                );
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            Err(error) if is_dns_error(&error) => {
                log::debug!("[http] {url_log}: DNS error={error}");
                return None;
            }
            Err(error) => {
                log::debug!("[http] {url_log}: error={error} attempt={attempt}");
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    }
    None
}

/// `Accept: application/json` — MusicBrainz/CoverArtArchive return XML
/// otherwise. Body parsing left to callers; same retry as `http_get_with_retry`.
pub fn http_get_json(url: &str) -> Option<String> {
    http_get_text_with_headers(
        url,
        &RequestHeaders::default(),
        &[("Accept", "application/json")],
        Duration::from_secs(TIMEOUT_SECS),
        true,
    )
}

/// Binary GET (cover images etc.); 1 MB cap.
pub fn http_get_bytes(url: &str) -> Option<Vec<u8>> {
    let http_response = curl_bytes(
        url,
        &RequestHeaders::default(),
        &[],
        1024 * 1024,
        Duration::from_secs(TIMEOUT_SECS),
        true,
    )
    .ok()?;
    (http_response.status == 200).then_some(http_response.bytes)
}

/// IPTV providers often require specific `User-Agent`/`Referer`; empty fields
/// fall back to the command default. Use [`RequestHeaders::default`] otherwise.
#[derive(Debug, Clone, Default)]
pub struct RequestHeaders<'a> {
    /// Override for the `User-Agent` header; empty falls back to the default.
    pub user_agent: &'a str,
    /// Override for the `Referer` header; empty omits the header.
    pub referer: &'a str,
}

impl<'a> RequestHeaders<'a> {
    fn is_empty(&self) -> bool {
        self.user_agent.is_empty() && self.referer.is_empty()
    }

    fn pairs(&self) -> Vec<(&str, &str)> {
        let mut pairs = Vec::with_capacity(2);
        if !self.user_agent.is_empty() {
            pairs.push(("User-Agent", self.user_agent));
        }
        if !self.referer.is_empty() {
            pairs.push(("Referer", self.referer));
        }
        pairs
    }
}

/// Reuses the shared command policy; retries once on transient errors.
pub fn http_get_with_headers(url: &str, headers: &RequestHeaders<'_>) -> Option<String> {
    if headers.is_empty() {
        return http_get_with_retry(url);
    }
    http_get_text_with_headers(url, headers, &[], Duration::from_secs(TIMEOUT_SECS), true)
}

/// Binary GET + per-request headers; 1 MB cap.
pub fn http_get_bytes_with_headers(url: &str, headers: &RequestHeaders<'_>) -> Option<Vec<u8>> {
    let http_response = curl_bytes(
        url,
        headers,
        &[],
        1024 * 1024,
        Duration::from_secs(TIMEOUT_SECS),
        true,
    )
    .ok()?;
    (http_response.status == 200).then_some(http_response.bytes)
}

/// GET a response into memory with caller-owned body cap and response metadata.
pub fn http_get_bytes_capped(
    url: &str,
    headers: &RequestHeaders<'_>,
    max_bytes: u64,
    timeout: Duration,
) -> Result<HttpResponseBytes, String> {
    http_get_bytes_capped_with_extra_headers(url, headers, &[], max_bytes, timeout)
}

/// GET a response into memory with caller-owned body cap, response metadata,
/// and additional per-call headers.
pub fn http_get_bytes_capped_with_extra_headers(
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    max_bytes: u64,
    timeout: Duration,
) -> Result<HttpResponseBytes, String> {
    curl_bytes(url, headers, extra_headers, max_bytes, timeout, true)
}

/// GET a streaming response with metadata and caller-owned body policy.
pub fn http_get_stream(
    url: &str,
    headers: &RequestHeaders<'_>,
    timeout: Duration,
) -> Result<HttpStreamResponse, String> {
    http_get_stream_with_extra_headers(url, headers, &[], timeout)
}

/// GET a streaming response with metadata, additional per-call headers, and
/// caller-owned body policy.
pub fn http_get_stream_with_extra_headers(
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<HttpStreamResponse, String> {
    curl_stream(url, headers, extra_headers, timeout, true)
}

/// Copy a response stream to a writer with cancellation and progress callbacks.
pub fn copy_stream_with_progress<W, C, P>(
    reader: &mut dyn Read,
    writer: &mut W,
    mut is_cancelled: C,
    mut on_chunk: P,
) -> Result<(), String>
where
    W: Write,
    C: FnMut() -> bool,
    P: FnMut(u64),
{
    let mut buf = [0_u8; 65_536];
    loop {
        if is_cancelled() {
            return Err("cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        writer.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        on_chunk(n as u64);
    }
}

fn http_get_text_with_headers(
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    timeout: Duration,
    follow_redirects: bool,
) -> Option<String> {
    let url_log = MediaLogTarget(url);
    for attempt in 0..2 {
        match curl_bytes(
            url,
            headers,
            extra_headers,
            MAX_TEXT_BODY_BYTES,
            timeout,
            follow_redirects,
        ) {
            Ok(resp) if resp.status < 500 => {
                if resp.status != 200 {
                    log::debug!("[http] {url_log}: non-200 status={}", resp.status);
                    return None;
                }
                return String::from_utf8(resp.bytes)
                    .inspect_err(|e| log::debug!("[http] {url_log}: utf8 body error={e}"))
                    .ok();
            }
            Ok(resp) => {
                log::debug!(
                    "[http] {url_log}: 5xx status={} attempt={attempt}",
                    resp.status
                );
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            Err(error) if is_dns_error(&error) => {
                log::debug!("[http] {url_log}: DNS error={error}");
                return None;
            }
            Err(error) => {
                log::debug!("[http] {url_log}: error={error} attempt={attempt}");
                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
    }
    None
}

fn curl_bytes(
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    max_bytes: u64,
    timeout: Duration,
    follow_redirects: bool,
) -> Result<HttpResponseBytes, String> {
    let mut response = curl_stream(url, headers, extra_headers, timeout, follow_redirects)?;
    let bytes = read_capped(response.reader.as_mut(), max_bytes)?;
    Ok(HttpResponseBytes {
        status: response.status,
        final_url: response.final_url,
        content_type: response.content_type,
        bytes,
    })
}

fn curl_stream(
    url: &str,
    headers: &RequestHeaders<'_>,
    extra_headers: &[(&str, &str)],
    timeout: Duration,
    follow_redirects: bool,
) -> Result<HttpStreamResponse, String> {
    validate_url(url)?;
    if timeout.is_zero() || timeout > Duration::from_secs(7200) {
        return Err("Invalid HTTP timeout".into());
    }
    // Custom headers may contain provider-specific secrets curl cannot recognize.
    // Do not forward them (or a private Referer) to an automatically redirected host.
    let follow = follow_redirects
        && headers.referer.is_empty()
        && extra_headers.iter().all(|(name, value)| {
            // These are fixed public negotiation headers used by metadata callers;
            // unknown/custom headers stay on the original host.
            name.eq_ignore_ascii_case("Accept")
                && matches!(*value, "application/json" | "text/html" | "*/*")
        });
    let config = request_config(url, headers, extra_headers)?;
    let mut command = base_curl(timeout);
    command.args(["--include", "--suppress-connect-headers"]);
    if follow {
        command.args([
            "--location",
            "--max-redirs",
            &DEFAULT_MAX_REDIRECTS.to_string(),
        ]);
    }
    command.args([
        "--proto-redir",
        if url
            .get(..6)
            .is_some_and(|v| v.eq_ignore_ascii_case("https:"))
        {
            "=https"
        } else {
            "=http,https"
        },
    ]);
    let transport = transport::Transport::new(command, config.as_bytes(), timeout)
        .map_err(|e| e.to_string())?;
    // This SAME reader goes into the response: any prefetched body bytes survive
    // header parsing. EOF verifies the child exit status, not just pipe closure.
    let mut reader = BufReader::new(transport);
    let parsed = read_final_headers(&mut reader, follow)?;
    Ok(HttpStreamResponse {
        status: parsed.status,
        final_url: url.to_owned(),
        content_type: parsed.content_type,
        content_length: parsed.content_length,
        reader: Box::new(reader),
    })
}
fn base_curl(timeout: Duration) -> Command {
    let mut command = Command::new(CURL);
    // --disable MUST be the first parameter. All untrusted values go through
    // stdin configuration; none are command-line arguments or environment values.
    command
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--globoff",
            "--proto",
            "=http,https",
            "--connect-timeout",
            &timeout
                .as_secs_f64()
                .min(FAST_TIMEOUT_SECS as f64)
                .to_string(),
            "--max-time",
            &timeout.as_secs_f64().to_string(),
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}
const MAX_REQUEST_CONFIG: usize = 2 * 1024 * 1024;
fn config_value(config: &mut String, name: &str, value: &str) -> Result<(), String> {
    if value.len() > MAX_REQUEST_CONFIG || value.contains('\0') {
        return Err("Invalid or excessive curl configuration value".into());
    }
    config.push_str(name);
    config.push_str(" = \"");
    for ch in value.chars() {
        match ch {
            '\\' => config.push_str("\\\\"),
            '"' => config.push_str("\\\""),
            '\r' => config.push_str("\\r"),
            '\n' => config.push_str("\\n"),
            '\t' => config.push_str("\\t"),
            '\u{000b}' => config.push_str("\\v"),
            other => config.push(other),
        }
        if config.len() > MAX_REQUEST_CONFIG {
            return Err("Curl configuration exceeds its limit".into());
        }
    }
    config.push_str("\"\n");
    if config.len() > MAX_REQUEST_CONFIG {
        return Err("Curl configuration exceeds its limit".into());
    }
    Ok(())
}
fn request_config(
    url: &str,
    headers: &RequestHeaders<'_>,
    extra: &[(&str, &str)],
) -> Result<String, String> {
    let mut config = String::new();
    config_value(&mut config, "url", url)?;
    config_value(&mut config, "request", "GET")?;
    let user_agent = if headers.user_agent.is_empty() {
        USER_AGENT
    } else {
        headers.user_agent
    };
    validate_header_value(user_agent)?;
    config_value(&mut config, "user-agent", user_agent)?;
    for (name, value) in headers.pairs().into_iter().chain(extra.iter().copied()) {
        if value.is_empty() {
            continue;
        }
        validate_header_name(name)?;
        validate_header_value(value)?;
        config_value(&mut config, "header", &format!("{name}: {value}"))?;
    }
    Ok(config)
}
fn validate_url(url: &str) -> Result<(), String> {
    if url.len() > 65536 || url.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("Invalid HTTP URL".into());
    }
    let (scheme, authority) = url
        .split_once("://")
        .ok_or("HTTP URL requires a scheme and authority")?;
    if !(scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
        || authority.is_empty()
        || authority.starts_with('/')
    {
        return Err("Only HTTP and HTTPS URLs are supported".into());
    }
    Ok(())
}
fn validate_header_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 256
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
    {
        return Err("HTTP header name is invalid".into());
    }
    Ok(())
}
fn validate_header_value(value: &str) -> Result<(), String> {
    if value.len() > 65536 || value.chars().any(|ch| ch.is_control() && ch != '\t') {
        return Err("HTTP header value is invalid".into());
    }
    Ok(())
}

fn read_final_headers(
    stdout: &mut impl Read,
    follow_redirects: bool,
) -> Result<CurlHeaders, String> {
    let mut total = 0_usize;
    for _ in 0..=usize::from(DEFAULT_MAX_REDIRECTS) + 8 {
        let headers = read_one_header_block(stdout)?;
        total += headers.len();
        if total > MAX_HEADER_BYTES {
            return Err("HTTP headers exceeded total byte limit".into());
        }
        let status = parse_status(&headers)?;
        let location = parse_header_value(&headers, "location");
        if (100..200).contains(&status)
            || (follow_redirects && (300..400).contains(&status) && location.is_some())
        {
            continue;
        }
        let content_type = parse_header_value(&headers, "content-type");
        let content_length = parse_header_value(&headers, "content-length")
            .and_then(|value| value.parse::<u64>().ok());
        return Ok(CurlHeaders {
            status,
            content_type,
            content_length,
        });
    }
    Err("Too many HTTP header blocks".into())
}

fn read_one_header_block(stdout: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut headers = Vec::new();
    let mut byte = [0_u8; 1];
    while headers.len() <= MAX_HEADER_BYTES {
        let n = stdout
            .read(&mut byte)
            .map_err(|e| format!("read curl headers: {e}"))?;
        if n == 0 {
            return Err("curl ended before HTTP headers".to_owned());
        }
        headers.push(byte[0]);
        if headers.ends_with(b"\r\n\r\n") || headers.ends_with(b"\n\n") {
            return Ok(headers);
        }
    }
    Err(format!(
        "HTTP headers exceeded {MAX_HEADER_BYTES} bytes cap"
    ))
}

fn parse_status(headers: &[u8]) -> Result<u16, String> {
    let text = String::from_utf8_lossy(headers);
    text.lines()
        .filter_map(|line| line.strip_prefix("HTTP/"))
        .filter_map(|rest| rest.split_whitespace().nth(1))
        .filter_map(|code| code.parse::<u16>().ok())
        .next_back()
        .ok_or_else(|| "missing HTTP status from curl headers".to_owned())
}

fn parse_header_value(headers: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(headers);
    text.lines().find_map(|line| {
        let (header_name, value) = line.split_once(':')?;
        header_name
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn read_capped(reader: &mut dyn Read, max_bytes: u64) -> Result<Vec<u8>, String> {
    // Use copy_stream_with_progress for larger downloads instead of allocating
    // arbitrary user-controlled caps in memory.
    if max_bytes > 64 * 1024 * 1024 {
        return Err("Buffered HTTP limit exceeds 64 MiB; use streaming copy".into());
    }
    let mut body = Vec::new();
    let mut limited = reader.take(max_bytes.saturating_add(1));
    limited
        .read_to_end(&mut body)
        .map_err(|e| format!("read curl body: {e}"))?;
    if body.len() as u64 > max_bytes {
        return Err(format!("response body exceeded {max_bytes} bytes cap"));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_identifies_biglinux_rust_components() {
        assert!(USER_AGENT.contains("BigLinuxRustApp"));
    }

    #[test]
    // Miri does not implement `posix_spawn`; normal `cargo test` still checks
    // that the system curl backend fails closed for invalid URLs.
    #[cfg_attr(miri, ignore)]
    fn retry_returns_none_for_invalid_url() {
        assert!(http_get_with_retry("not-a-url").is_none());
    }

    #[test]
    // Miri does not implement `posix_spawn`; normal `cargo test` still checks
    // that the fast system curl path fails closed for invalid URLs.
    #[cfg_attr(miri, ignore)]
    fn fast_returns_none_for_invalid_url() {
        assert!(http_get_fast("not-a-url").is_none());
    }

    #[test]
    fn fast_timeout_is_shorter() {
        const { assert!(FAST_TIMEOUT_SECS < TIMEOUT_SECS) };
        const { assert!(FAST_TIMEOUT_SECS >= 3) };
        const { assert!(FAST_TIMEOUT_SECS <= 10) };
    }

    #[test]
    fn fast_global_timeout_reasonable() {
        const { assert!(FAST_GLOBAL_TIMEOUT_SECS >= TIMEOUT_SECS) };
        const { assert!(FAST_GLOBAL_TIMEOUT_SECS <= 60) };
    }

    #[test]
    fn dns_error_detected() {
        assert!(is_dns_error("curl exited with code 6"));
    }

    #[test]
    fn timeout_error_not_dns() {
        assert!(!is_dns_error("curl exited with code 28"));
    }

    #[test]
    // Miri does not implement `posix_spawn`; normal `cargo test` still checks
    // that the binary curl path fails closed for invalid URLs.
    #[cfg_attr(miri, ignore)]
    fn bytes_returns_none_for_invalid_url() {
        assert!(http_get_bytes("not-a-url").is_none());
    }

    #[test]
    // Empty headers intentionally delegate into the system curl retry path.
    #[cfg_attr(miri, ignore)]
    fn empty_headers_delegate_to_retry() {
        let headers = RequestHeaders::default();
        assert!(headers.is_empty());
        assert!(http_get_with_headers("not-a-url", &headers).is_none());
    }

    #[test]
    fn request_headers_default_is_empty() {
        let h = RequestHeaders::default();
        assert!(h.user_agent.is_empty());
        assert!(h.referer.is_empty());
    }

    #[test]
    // Miri does not implement `posix_spawn`; normal `cargo test` still checks
    // that the header-aware binary curl path fails closed for invalid URLs.
    #[cfg_attr(miri, ignore)]
    fn bytes_with_headers_returns_none_for_invalid_url() {
        assert!(http_get_bytes_with_headers("not-a-url", &RequestHeaders::default()).is_none());
    }

    #[test]
    fn rejects_header_newlines() {
        assert!(validate_header_value("Bearer ok\nInjected: yes").is_err());
    }

    #[test]
    fn rejects_invalid_header_name() {
        assert!(validate_header_name("Bad:Header").is_err());
    }

    #[test]
    fn parses_status_from_last_http_header_block() {
        let headers =
            b"HTTP/1.1 100 Continue\r\n\r\nHTTP/2 200\r\ncontent-type: text/plain\r\n\r\n";
        assert_eq!(parse_status(headers).unwrap(), 200);
    }

    #[test]
    fn parses_header_values_case_insensitively() {
        let headers = b"HTTP/2 200\r\nContent-Type: text/plain\r\nContent-Length: 42\r\n\r\n";
        assert_eq!(
            parse_header_value(headers, "content-type"),
            Some("text/plain".to_owned())
        );
        assert_eq!(
            parse_header_value(headers, "content-length"),
            Some("42".to_owned())
        );
    }
}

#[cfg(test)]
mod transport_contract_tests {
    use super::*;
    #[test]
    fn config_keeps_secrets_off_argv_and_treats_at_sign_literally() {
        let config = request_config(
            "https://example.invalid/?token=secret",
            &RequestHeaders::default(),
            &[("Authorization", "Bearer secret")],
        )
        .unwrap();
        assert!(config.contains("header = \"Authorization: Bearer secret\""));
        let args: Vec<_> = base_curl(Duration::from_secs(10))
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "--disable");
        assert!(!args.iter().any(|arg| arg.contains("secret")));
    }
    #[test]
    fn buffered_header_parser_preserves_prefetched_body() {
        let bytes = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody";
        let mut reader = BufReader::new(std::io::Cursor::new(bytes));
        assert_eq!(read_final_headers(&mut reader, false).unwrap().status, 200);
        assert_eq!(read_capped(&mut reader, 4).unwrap(), b"body");
    }
    #[test]
    fn invalid_headers_and_unbounded_caps_are_rejected() {
        assert!(validate_header_name("Bad:Name").is_err());
        assert!(validate_header_value("bad\0value").is_err());
        assert!(read_capped(&mut std::io::Cursor::new([]), u64::MAX).is_err());
    }
}
