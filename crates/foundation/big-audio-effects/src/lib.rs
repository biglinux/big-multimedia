// SPDX-License-Identifier: MIT

#![deny(missing_docs)]
#![forbid(unsafe_code)]

//! Shared audio-effect contracts for the BigLinux media players.
//!
//! This crate centralizes the small, hard-to-change pieces of an audio
//! processing chain so that `big-audio-player` and `big-video-player`
//! produce identical results from identical user input.
//!
//! The contracts captured here include:
//!
//! - LADSPA plugin paths, control-port ordering and noise-reduction model
//!   identifiers ([`noise_reduction`]).
//! - The canonical 10-band equalizer center frequencies and labels
//!   ([`equalizer`]).
//! - Derived gate and compressor curves plus FFmpeg filter string
//!   formatters ([`dynamics`]).
//!
//! Treat the values exported here as part of a public on-disk format:
//! changing them invalidates user presets and breaks PipeWire graphs.
//!
//! # Examples
//!
//! Build an FFmpeg compressor filter string from a user slider:
//!
//! ```
//! use big_audio_effects::dynamics::ffmpeg_acompressor_filter;
//!
//! let filter = ffmpeg_acompressor_filter(0.5);
//! assert!(filter.starts_with("acompressor="));
//! ```
//!
//! Look up the labels shown to users in the equalizer UI:
//!
//! ```
//! use big_audio_effects::equalizer::{EQ_BAND_COUNT, EQ_BAND_LABELS};
//!
//! assert_eq!(EQ_BAND_LABELS.len(), EQ_BAND_COUNT);
//! assert_eq!(EQ_BAND_LABELS[0], "31");
//! ```

/// Derived gate and compressor curves and FFmpeg filter formatters.
pub mod dynamics;
/// Canonical 10-band equalizer frequencies and labels.
pub mod equalizer;
/// Noise-reduction model identifiers and LADSPA control contracts.
pub mod noise_reduction;
