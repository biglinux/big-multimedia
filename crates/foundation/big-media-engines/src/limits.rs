// SPDX-License-Identifier: MIT

//! Parser size and count limits.
//!
//! Every parser in this crate accepts a [`ParseLimits`] value so hostile
//! or accidentally-huge inputs cannot drive the process out of memory.
//! Defaults are conservative and match the BigLinux plan document
//! `M2.9`. Callers can pick the appropriate preset
//! ([`ParseLimits::SUBTITLE`], [`ParseLimits::PLAYLIST`],
//! [`ParseLimits::FEED`]) or construct their own.

/// Upper bounds applied uniformly across the parsers.
///
/// A parser refuses input larger than [`ParseLimits::max_bytes`] before
/// allocating, and stops with an error once the produced record count
/// would exceed [`ParseLimits::max_records`]. The struct is `Copy`, so
/// it can be passed by value without an explicit clone.
///
/// # Examples
///
/// Build a tight custom limit for a quick smoke test:
///
/// ```
/// use big_media_engines::ParseLimits;
///
/// let small = ParseLimits { max_bytes: 4 * 1024, max_records: 32 };
/// assert_eq!(small.max_bytes, 4 * 1024);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParseLimits {
    /// Maximum accepted input length, in bytes.
    pub max_bytes: usize,
    /// Maximum number of produced records (subtitle cues, playlist
    /// entries, feed items, etc.).
    pub max_records: usize,
}

impl ParseLimits {
    /// Subtitle defaults: 2 MiB of bytes and 100 000 cues.
    pub const SUBTITLE: Self = Self {
        max_bytes: 2 * 1024 * 1024,
        max_records: 100_000,
    };

    /// M3U / playlist defaults: 8 MiB of bytes and 50 000 entries.
    pub const PLAYLIST: Self = Self {
        max_bytes: 8 * 1024 * 1024,
        max_records: 50_000,
    };

    /// Feed (RSS / Atom) defaults: 4 MiB of bytes and 5 000 items.
    pub const FEED: Self = Self {
        max_bytes: 4 * 1024 * 1024,
        max_records: 5_000,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_defaults_match_plan() {
        assert_eq!(ParseLimits::SUBTITLE.max_bytes, 2 * 1024 * 1024);
        assert_eq!(ParseLimits::SUBTITLE.max_records, 100_000);
    }

    #[test]
    fn playlist_defaults_match_plan() {
        assert_eq!(ParseLimits::PLAYLIST.max_bytes, 8 * 1024 * 1024);
    }

    #[test]
    fn feed_defaults_match_plan() {
        assert_eq!(ParseLimits::FEED.max_bytes, 4 * 1024 * 1024);
    }
}
