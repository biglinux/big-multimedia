// SPDX-License-Identifier: MIT

//! Subtitle data carriers.
//!
//! These types describe the result and error model used by the
//! subtitle parsers. They carry no formatting state and are safe to
//! pass between threads.

/// Subtitle formats recognised by the [`crate::subtitle::parse`]
/// dispatcher.
///
/// Today only [`SubtitleFormat::SubRip`] is implemented; the enum is
/// `non_exhaustive` in spirit (new variants may be added in semver
/// minor releases and callers should pattern-match accordingly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubtitleFormat {
    /// SubRip subtitle (`.srt`).
    SubRip,
}

/// One parsed subtitle cue.
///
/// Produced in source order by [`crate::subtitle::parse_srt`]. Markup
/// (`<i>`, `<b>`, `<u>`, `<font>`) is stripped from [`Cue::text`] at
/// the parser layer so consumers can render the string verbatim.
///
/// # Examples
///
/// ```
/// use big_media_engines::ParseLimits;
/// use big_media_engines::subtitle::parse_srt;
///
/// let src = b"1\n00:00:01,000 --> 00:00:02,000\nhello\n";
/// let cues = parse_srt(src, ParseLimits::SUBTITLE).unwrap();
/// let first = &cues[0];
/// assert_eq!(first.index, 1);
/// assert_eq!(first.text, "hello");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    /// 1-indexed cue identifier as written in the source file.
    pub index: u32,
    /// Start timestamp of the cue.
    pub start: CueTime,
    /// End timestamp of the cue.
    pub end: CueTime,
    /// Plain cue text. Simple HTML-style markup is stripped at parse
    /// time so the value can be rendered as-is.
    pub text: String,
}

/// A cue timestamp expressed in whole milliseconds from the start of
/// the file.
///
/// The wrapping `u64` is `pub` so callers can perform arithmetic
/// without extra getters; use [`CueTime::from_hms_millis`] to build
/// values in a self-documenting way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CueTime(
    /// Milliseconds from the start of the file.
    pub u64,
);

impl CueTime {
    /// Build a [`CueTime`] from `HH:MM:SS,ms` components.
    ///
    /// No range checking is performed; callers are expected to provide
    /// already-validated values (typical use is inside a parser, which
    /// validates the textual ranges first).
    #[must_use]
    pub const fn from_hms_millis(hours: u64, minutes: u64, seconds: u64, millis: u64) -> Self {
        Self(hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + millis)
    }
}

/// Failure modes produced by the subtitle parsers.
///
/// Returned by [`crate::subtitle::parse`] and
/// [`crate::subtitle::parse_srt`]. All variants implement
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
    /// The produced cue count exceeded
    /// [`crate::limits::ParseLimits::max_records`].
    TooManyCues {
        /// Number of cues collected before the failure.
        count: usize,
        /// Configured record limit.
        limit: usize,
    },
    /// A cue timing line did not match the `HH:MM:SS,ms --> HH:MM:SS,ms`
    /// shape.
    MalformedTiming {
        /// 1-indexed line number in the source.
        line_number: u32,
    },
    /// A cue index line was not a non-negative decimal integer.
    MalformedIndex {
        /// 1-indexed line number in the source.
        line_number: u32,
    },
    /// The input was neither valid UTF-8 nor decodable as Latin-1
    /// (in practice this variant is currently unreachable because the
    /// SRT parser falls back to Latin-1 unconditionally, but the
    /// variant is exposed for parsers that may apply stricter rules).
    InvalidEncoding,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputTooLarge { len, limit } => {
                write!(f, "input too large: {len} bytes, limit {limit}")
            }
            Self::TooManyCues { count, limit } => {
                write!(f, "too many cues: {count}, limit {limit}")
            }
            Self::MalformedTiming { line_number } => {
                write!(f, "malformed timing on line {line_number}")
            }
            Self::MalformedIndex { line_number } => {
                write!(f, "malformed cue index on line {line_number}")
            }
            Self::InvalidEncoding => write!(f, "input is not UTF-8 or Latin-1"),
        }
    }
}

impl std::error::Error for ParseError {}
