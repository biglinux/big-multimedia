//! Unified error type for `big-media-runtime`.
//!
//! Single `Error` enum with `thiserror`-generated `Display` impls keeps
//! every fallible API in the crate honest about kind + context. The
//! `#[from]` conversions let propagation with `?` work for the common
//! low-level error sources (I/O, SQLite, JSON, parsing, UTF-8).
//!
//! Display strings of every variant are user-visible: they may end up
//! in toasts via `to_string()`, so they read as full English sentences
//! prefixed with the kind so callers can grep logs.

use thiserror::Error;

/// Crate-wide error type. Covers every fallible boundary the runtime
/// reaches: filesystem, SQLite, JSON, mpv, ffmpeg, Chromecast.
///
/// `#[non_exhaustive]` keeps adding new variants from being a semver
/// break — consumers must use a wildcard arm.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Filesystem / process / pipe I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// SQLite driver error from the music library / metadata cache.
    #[cfg(feature = "db")]
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// JSON parse failure (ffprobe output, settings, profile files).
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// Borrowed UTF-8 conversion failure (subprocess stdout slices).
    #[error("utf-8 (borrowed): {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// Owned UTF-8 conversion failure (`String::from_utf8`).
    #[error("utf-8 (owned): {0}")]
    FromUtf8(#[from] std::string::FromUtf8Error),

    /// Integer parse failure (numeric tokens from text formats).
    #[error("parse int: {0}")]
    ParseInt(#[from] std::num::ParseIntError),

    /// Float parse failure (duration / framerate numbers).
    #[error("parse float: {0}")]
    ParseFloat(#[from] std::num::ParseFloatError),

    /// mpv backend textual error. mpv's C API returns short strings —
    /// preserve them verbatim.
    #[error("mpv: {0}")]
    Mpv(String),

    /// ffmpeg / ffprobe subprocess failure.
    ///
    /// `code` formatted as a bare integer or the literal `unknown` when the
    /// process was killed before exiting; avoids exposing Rust's
    /// `Option`/`Some(_)` debug syntax to user-facing toasts.
    #[error(
        "ffmpeg: {command} exited with code {}: {message}",
        .code.map_or_else(|| "unknown".to_owned(), |c| c.to_string())
    )]
    Ffmpeg {
        /// Binary name (`"ffmpeg"` / `"ffprobe"`) — `&'static str`.
        command: &'static str,
        /// Exit code, when the process actually exited.
        code: Option<i32>,
        /// Stderr summary or our own description.
        message: String,
    },

    /// Chromecast discovery / casting / network error.
    #[error("chromecast: {0}")]
    Chromecast(String),

    /// Caller passed an argument that doesn't make sense in context.
    #[error("invalid argument {what}: {why}")]
    InvalidArgument {
        /// Static argument label (parameter name or symbolic field).
        what: &'static str,
        /// Human reason why it was rejected.
        why: String,
    },

    /// Resource (file / row / id) not present.
    #[error("not found: {resource}")]
    NotFound {
        /// Static resource kind label.
        resource: &'static str,
    },

    /// Operation cancelled cooperatively (queue abort, user stop).
    #[error("operation cancelled")]
    Cancelled,

    /// Catch-all internal error. Prefer a typed variant when possible.
    #[error("internal: {0}")]
    Internal(String),
}

impl Error {
    /// Construct an ffmpeg subprocess failure from binary + exit + msg.
    #[must_use]
    pub fn ffmpeg(command: &'static str, code: Option<i32>, message: impl Into<String>) -> Self {
        Self::Ffmpeg {
            command,
            code,
            message: message.into(),
        }
    }

    /// Construct an mpv textual error.
    #[must_use]
    pub fn mpv(message: impl Into<String>) -> Self {
        Self::Mpv(message.into())
    }

    /// Construct a Chromecast / network error.
    #[must_use]
    pub fn chromecast(message: impl Into<String>) -> Self {
        Self::Chromecast(message.into())
    }

    /// Construct an internal error (last resort).
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Construct an `InvalidArgument` with static `what` + dynamic `why`.
    #[must_use]
    pub fn invalid_argument(what: &'static str, why: impl Into<String>) -> Self {
        Self::InvalidArgument {
            what,
            why: why.into(),
        }
    }

    /// Construct a `NotFound` with a static resource label.
    #[must_use]
    pub fn not_found(resource: &'static str) -> Self {
        Self::NotFound { resource }
    }
}

/// Transitional `From<String>` to land the migration without rewriting
/// every `.map_err(|e| format!(...))` callsite in one pass. Keeps the
/// string as `Internal(...)` so logs still surface the original text.
impl From<String> for Error {
    fn from(s: String) -> Self {
        Self::Internal(s)
    }
}

impl Error {
    /// Deprecated wrapper that flags the transitional `From<String>` path
    /// at call sites. Prefer [`Error::internal`] or a typed variant.
    ///
    /// caveman: rustc forbids `#[deprecated]` on trait-impl methods, so
    /// we surface the warning through this inherent shim that mirrors
    /// the impl. Callers writing `Error::from_string(s)` get the lint;
    /// existing `.into()` callsites remain warning-free until migrated
    /// manually.
    #[deprecated(note = "use Error::internal() or a typed variant")]
    #[must_use]
    pub fn from_string(s: String) -> Self {
        Self::Internal(s)
    }
}

// NOTE: `impl From<&str>` is intentionally absent. `&str` literals leaking
// into typed-error sites obscure real failure modes; callers with a `&str`
// must use `Error::internal(s)` or `.to_string().into()` explicitly.

/// Crate-wide `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_io() {
        let e: Error = std::io::Error::other("disk full").into();
        assert!(e.to_string().starts_with("io: "));
        assert!(matches!(e, Error::Io(_)));
    }

    #[test]
    #[cfg(feature = "db")]
    fn from_sqlite() {
        let e: Error = rusqlite::Error::QueryReturnedNoRows.into();
        assert!(e.to_string().starts_with("sqlite: "));
        assert!(matches!(e, Error::Sqlite(_)));
    }

    #[test]
    fn from_json() {
        let parse: serde_json::Error = serde_json::from_str::<i32>("not json").unwrap_err();
        let e: Error = parse.into();
        assert!(e.to_string().starts_with("json: "));
        assert!(matches!(e, Error::Json(_)));
    }

    #[test]
    fn from_utf8() {
        let bad: Vec<u8> = vec![0xff, 0xfe, 0xfd];
        let inner = std::str::from_utf8(&bad).unwrap_err();
        let e: Error = inner.into();
        // caveman: borrowed vs owned utf-8 must be greppable
        assert!(e.to_string().starts_with("utf-8 (borrowed): "));
        assert!(matches!(e, Error::Utf8(_)));
    }

    #[test]
    fn from_from_utf8() {
        let bad = vec![0xff, 0xfe, 0xfd];
        let inner = String::from_utf8(bad).unwrap_err();
        let e: Error = inner.into();
        assert!(e.to_string().starts_with("utf-8 (owned): "));
        assert!(matches!(e, Error::FromUtf8(_)));
    }

    #[test]
    fn from_parse_int() {
        let inner = "x".parse::<i32>().unwrap_err();
        let e: Error = inner.into();
        assert!(e.to_string().starts_with("parse int: "));
        assert!(matches!(e, Error::ParseInt(_)));
    }

    #[test]
    fn from_parse_float() {
        let inner = "x".parse::<f64>().unwrap_err();
        let e: Error = inner.into();
        assert!(e.to_string().starts_with("parse float: "));
        assert!(matches!(e, Error::ParseFloat(_)));
    }

    #[test]
    #[allow(deprecated)]
    fn from_string_internal() {
        let e: Error = "oops".to_string().into();
        assert!(e.to_string().starts_with("internal: "));
        assert!(matches!(e, Error::Internal(_)));
    }

    #[test]
    fn display_mpv() {
        let e = Error::mpv("init failed");
        assert_eq!(e.to_string(), "mpv: init failed");
    }

    #[test]
    fn display_ffmpeg() {
        let e = Error::ffmpeg("ffmpeg", Some(1), "x265: encoder error");
        let s = e.to_string();
        assert!(s.starts_with("ffmpeg: ffmpeg"));
        assert!(s.contains("code 1"));
        assert!(s.contains("x265"));
        // Contract: never expose Rust's Option/Some/None debug syntax to
        // user-facing toasts.
        assert!(
            !s.contains("Some("),
            "Display must not contain Rust debug syntax: {s}"
        );
    }

    #[test]
    fn display_ffmpeg_unknown_code() {
        let e = Error::ffmpeg("ffprobe", None, "killed by signal");
        let s = e.to_string();
        assert!(
            s.contains("code unknown"),
            "expected `code unknown`, got: {s}"
        );
        assert!(
            !s.contains("None"),
            "Display must not contain Rust debug syntax: {s}"
        );
    }

    #[test]
    fn display_chromecast() {
        let e = Error::chromecast("device unreachable");
        assert_eq!(e.to_string(), "chromecast: device unreachable");
    }

    #[test]
    fn display_invalid_argument() {
        let e = Error::invalid_argument("seek_secs", "negative");
        assert!(e.to_string().starts_with("invalid argument seek_secs:"));
    }

    #[test]
    fn display_not_found() {
        let e = Error::not_found("track");
        assert_eq!(e.to_string(), "not found: track");
    }

    #[test]
    fn display_cancelled() {
        assert_eq!(Error::Cancelled.to_string(), "operation cancelled");
    }

    #[test]
    fn display_internal() {
        let e = Error::internal("boom");
        assert_eq!(e.to_string(), "internal: boom");
    }
}
