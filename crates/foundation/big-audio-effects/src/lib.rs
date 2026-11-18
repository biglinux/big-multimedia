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
//!
//! Saved settings hold equalizer gains by band index and the noise-reduction
//! model by its index in [`noise_reduction::MODELS`], so reordering either
//! changes what a user's saved settings mean. The plugin paths, labels and
//! control indexes must match the installed LADSPA plugins: mpv's `af` chain
//! loads them by these names.
//!
//! # Examples
//!
//! Look up the labels shown to users in the equalizer UI:
//!
//! ```
//! use big_audio_effects::equalizer::{EQ_BAND_COUNT, EQ_BAND_LABELS};
//!
//! assert_eq!(EQ_BAND_LABELS.len(), EQ_BAND_COUNT);
//! assert_eq!(EQ_BAND_LABELS[0], "31");
//! ```

/// Canonical 10-band equalizer frequencies and labels.
pub mod equalizer;
pub mod noise_reduction;
