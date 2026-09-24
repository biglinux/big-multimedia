// SPDX-License-Identifier: MIT

//! Display-free media helpers shared by the BigLinux media stack: FFmpeg
//! output parsing, media file-type knowledge, output naming for conversion
//! queues, the play-queue model and (feature `radio-browser`) the
//! Radio-Browser API response parser.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

/// FFmpeg text-output parsers.
pub mod ffmpeg;
/// Media file-type knowledge: audio/video extension sets + path predicates.
pub mod media_files;
/// Output filename helpers for conversion queues.
pub mod output_paths;
pub mod queue;
/// Radio-Browser API response parser (feature `radio-browser`).
#[cfg(feature = "radio-browser")]
pub mod radio_browser;
