//! Unified error type for this crate.
//!
//! Single `Error` enum with `thiserror`-generated `Display` impls gives
//! every fallible API in the crate an explicit kind and context. The
//! `#[from]` conversions let propagation with `?` work for the common
//! low-level error sources (I/O, SQLite, JSON, parsing, UTF-8).
//!
//! Display strings of every variant are user-visible: they may end up
//! in notices via `to_string()`, so they read as full English sentences
//! prefixed with the kind so callers can grep logs.

use thiserror::Error;

/// Crate-wide error type. Covers every fallible boundary the crate
/// reaches: filesystem, SQLite, JSON, subprocesses and mpv.
///
/// `#[non_exhaustive]` keeps adding new variants from being a semver
/// break; consumers must use a wildcard arm.
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

    /// mpv backend textual error. mpv's C API returns short strings,
    /// kept verbatim.
    #[error("mpv: {0}")]
    Mpv(String),

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
    /// Construct an mpv textual error.
    #[must_use]
    pub fn mpv(message: impl Into<String>) -> Self {
        Self::Mpv(message.into())
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

/// Lets `.map_err(|e| format!(...))` call sites use `?`; the text lands in
/// `Internal`.
impl From<String> for Error {
    fn from(s: String) -> Self {
        Self::Internal(s)
    }
}

// No `impl From<&str>`: `&str` literals leaking
// into typed-error sites obscure real failure modes; callers with a `&str`
// must use `Error::internal(s)` or `.to_string().into()` explicitly.

/// Crate-wide `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;
