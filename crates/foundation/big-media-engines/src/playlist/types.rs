// SPDX-License-Identifier: MIT

//! Playlist data carriers.
//!
//! These types describe the output and error model of the playlist
//! parsers. They carry no formatting state and are safe to share
//! between threads.

use std::path::PathBuf;

/// Playlist formats recognised by [`crate::playlist::parse`].
///
/// M3U and M3U8 are parsed by the same backend; the variant only
/// records the source extension for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaylistFormat {
    /// Legacy M3U (Latin-1 / locale encoding tolerated).
    M3u,
    /// M3U8 (UTF-8 with `#EXT` directives).
    M3u8,
}

/// One playlist entry: either a local file path or a remote URL, plus
/// optional metadata harvested from a preceding `#EXTINF` line.
///
/// Produced by [`crate::playlist::parse_m3u`] in declared order.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistEntry {
    /// Resolved target ([`PlaylistTarget::Path`] or
    /// [`PlaylistTarget::Url`]).
    pub target: PlaylistTarget,
    /// Optional duration in seconds, parsed from `#EXTINF`.
    pub duration_secs: Option<f64>,
    /// Optional human-readable title, parsed from `#EXTINF`.
    pub title: Option<String>,
}

/// Resolved target of a [`PlaylistEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistTarget {
    /// Local filesystem path, already resolved against the playlist's
    /// base directory.
    Path(PathBuf),
    /// Remote URL (`http`, `https`, `rtsp`, `rtmp`, `mms`, `file`).
    Url(String),
}

/// Result of [`crate::playlist::parse_m3u`]: an ordered list of
/// [`PlaylistEntry`] values.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use big_media_engines::ParseLimits;
/// use big_media_engines::playlist::{parse_m3u, Playlist, PlaylistTarget};
///
/// let bytes = b"#EXTM3U\n#EXTINF:90,Track\nsong.mp3\n";
/// let pl: Playlist = parse_m3u(bytes, Path::new("/music"), ParseLimits::PLAYLIST)?;
/// for entry in &pl.entries {
///     match &entry.target {
///         PlaylistTarget::Path(p) => println!("local: {}", p.display()),
///         PlaylistTarget::Url(u) => println!("remote: {u}"),
///     }
/// }
/// # Ok::<(), big_media_engines::playlist::ParseError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Playlist {
    /// Entries in the order they appeared in the source file.
    pub entries: Vec<PlaylistEntry>,
}

/// Failure modes produced by the playlist parsers.
///
/// Returned by [`crate::playlist::parse`] and
/// [`crate::playlist::parse_m3u`]. All variants implement
/// [`std::fmt::Display`] and [`std::error::Error`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The input exceeded [`crate::limits::ParseLimits::max_bytes`].
    InputTooLarge {
        /// Actual input length, in bytes.
        len: usize,
        /// Configured byte limit.
        limit: usize,
    },
    /// The produced entry count exceeded
    /// [`crate::limits::ParseLimits::max_records`].
    TooManyEntries {
        /// Number of entries that triggered the failure.
        count: usize,
        /// Configured record limit.
        limit: usize,
    },
    /// A relative path escaped the playlist's base directory (e.g.
    /// `../../etc/passwd`).
    PathTraversal {
        /// The offending line, copied verbatim from the playlist.
        offender: String,
    },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputTooLarge { len, limit } => {
                write!(f, "playlist too large: {len} bytes, limit {limit}")
            }
            Self::TooManyEntries { count, limit } => {
                write!(f, "playlist too many entries: {count}, limit {limit}")
            }
            Self::PathTraversal { offender } => {
                write!(f, "playlist path escapes base: {offender}")
            }
        }
    }
}

impl std::error::Error for ParseError {}
