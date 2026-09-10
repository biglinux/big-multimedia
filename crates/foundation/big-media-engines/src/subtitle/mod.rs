// SPDX-License-Identifier: MIT

//! Subtitle parsers.
//!
//! Today only SubRip (`.srt`) is implemented via [`parse_srt`].
//! Additional formats can be added by mirroring the SRT layout: a
//! variant in [`SubtitleFormat`], a private `parse_<fmt>` module, and a
//! match arm in the [`parse`] dispatcher.
//!
//! Parsed cues are reported as [`Cue`] values with millisecond-precise
//! [`CueTime`] timestamps. Failures are surfaced via [`ParseError`].

mod parse_srt;
mod types;

pub use parse_srt::parse_srt;
pub use types::{Cue, CueTime, ParseError, SubtitleFormat};

use crate::limits::ParseLimits;

/// Parse subtitle bytes using an explicit [`SubtitleFormat`] hint.
///
/// This is a thin dispatcher over the per-format parsers. For SubRip
/// input it delegates directly to [`parse_srt`].
///
/// # Errors
///
/// Returns a [`ParseError`] when the input violates the supplied
/// [`ParseLimits`] (too large, too many cues) or is syntactically
/// invalid for the declared format.
///
/// # Examples
///
/// ```
/// use big_media_engines::ParseLimits;
/// use big_media_engines::subtitle::{self, SubtitleFormat};
///
/// let bytes = b"1\n00:00:01,000 --> 00:00:02,500\nHi\n";
/// let cues = subtitle::parse(SubtitleFormat::SubRip, bytes, ParseLimits::SUBTITLE)?;
/// assert_eq!(cues.len(), 1);
/// assert_eq!(cues[0].text, "Hi");
/// # Ok::<(), subtitle::ParseError>(())
/// ```
pub fn parse(
    format: SubtitleFormat,
    bytes: &[u8],
    limits: ParseLimits,
) -> Result<Vec<Cue>, ParseError> {
    match format {
        SubtitleFormat::SubRip => parse_srt(bytes, limits),
    }
}
