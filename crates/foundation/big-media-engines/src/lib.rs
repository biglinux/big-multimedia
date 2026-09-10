// SPDX-License-Identifier: MIT

//! Pure-Rust media parsers used by BigLinux apps.
//!
//! This crate provides display-free, panic-free parsers for the media
//! container formats used by the BigLinux media stack:
//!
//! - [`subtitle`] — SubRip (`.srt`) cues; see [`subtitle::parse_srt`].
//! - [`playlist`] — M3U / M3U8 playlists; see [`playlist::parse_m3u`].
//!
//! # Design
//!
//! Every parser accepts an explicit [`ParseLimits`] value so hostile
//! inputs cannot drive the process out of memory. All errors are
//! reported as typed enums (`subtitle::ParseError`,
//! `playlist::ParseError`); no parser ever panics on untrusted input
//! and no parser performs I/O.
//!
//! # Examples
//!
//! Parse a subtitle file:
//!
//! ```
//! use big_media_engines::ParseLimits;
//! use big_media_engines::subtitle::{self, SubtitleFormat};
//!
//! let bytes = b"1\n00:00:01,000 --> 00:00:02,000\nHello\n";
//! let cues = subtitle::parse(SubtitleFormat::SubRip, bytes, ParseLimits::SUBTITLE)
//!     .expect("valid SRT");
//! assert_eq!(cues.len(), 1);
//! ```

#![deny(missing_docs)]
#![forbid(unsafe_code)]

/// FFmpeg text-output parsers.
pub mod ffmpeg;
pub mod limits;
/// Media file-type knowledge: audio/video extension sets + path predicates.
pub mod media_files;
/// Output filename helpers for conversion queues.
pub mod output_paths;
pub mod playlist;
pub mod queue;
/// Radio-Browser API response parser (feature `radio-browser`).
#[cfg(feature = "radio-browser")]
pub mod radio_browser;
pub mod subtitle;

pub use limits::ParseLimits;
