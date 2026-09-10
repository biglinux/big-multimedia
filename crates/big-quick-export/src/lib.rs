//! Quick single-file export / re-encode helpers for the BigLinux media players.
//!
//! Extracted from the standalone converter (now a separate project): the audio
//! and video players embed only the in-place clip / convert / export path, which
//! lives here.
//!
//! - [`export`] drives a one-file export through the runtime FFmpeg queue.
//! - [`encode_options`] assembles the container / codec / quality option
//!   vocabulary shared with the (external) converter sidebar.
//! - [`converter`] re-exports the runtime FFmpeg / queue / profile types the
//!   above build on.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

// Internal re-export for `crate::i18n` paths. Not part of the public API.
pub(crate) use big_media_runtime::i18n;

pub mod combos;
pub mod converter;
pub mod encode_options;
pub mod export;
