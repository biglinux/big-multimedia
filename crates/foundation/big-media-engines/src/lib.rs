// SPDX-License-Identifier: MIT

//! Display-free media helpers shared by the BigLinux media stack: media
//! file-type knowledge, the play-queue model and (feature `radio-browser`) the
//! Radio-Browser API response parser.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

/// Media file-type knowledge: audio/video extension sets + path predicates.
pub mod media_files;
pub mod queue;
/// Radio-Browser API response parser (feature `radio-browser`).
#[cfg(feature = "radio-browser")]
pub mod radio_browser;
