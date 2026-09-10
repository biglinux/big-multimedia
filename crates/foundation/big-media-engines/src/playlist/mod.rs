// SPDX-License-Identifier: MIT

//! Playlist parsers.
//!
//! Currently only M3U / M3U8 is implemented via [`parse_m3u`]. The
//! [`parse`] dispatcher accepts a [`PlaylistFormat`] hint so additional
//! formats can be added without breaking callers.
//!
//! Parsed playlists are returned as a [`Playlist`] containing
//! [`PlaylistEntry`] values; failures are reported as [`ParseError`].

mod parse_m3u;
mod types;

pub use parse_m3u::parse_m3u;
pub use types::{ParseError, Playlist, PlaylistEntry, PlaylistFormat, PlaylistTarget};

use crate::limits::ParseLimits;
use std::path::Path;

/// Parse playlist bytes using an explicit [`PlaylistFormat`] hint.
///
/// Relative paths inside the playlist are resolved against `base_dir`
/// and rejected when they would escape it.
///
/// # Errors
///
/// See [`ParseError`].
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use big_media_engines::ParseLimits;
/// use big_media_engines::playlist::{self, PlaylistFormat};
///
/// let bytes = b"#EXTM3U\nfile.mp3\n";
/// let pl = playlist::parse(
///     PlaylistFormat::M3u,
///     bytes,
///     Path::new("/home/u/Music"),
///     ParseLimits::PLAYLIST,
/// )?;
/// assert_eq!(pl.entries.len(), 1);
/// # Ok::<(), playlist::ParseError>(())
/// ```
pub fn parse(
    format: PlaylistFormat,
    bytes: &[u8],
    base_dir: &Path,
    limits: ParseLimits,
) -> Result<Playlist, ParseError> {
    match format {
        PlaylistFormat::M3u | PlaylistFormat::M3u8 => parse_m3u(bytes, base_dir, limits),
    }
}
