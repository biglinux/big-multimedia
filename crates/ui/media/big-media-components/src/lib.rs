// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]

//! Reusable media UI components for BigLinux Rust applications.
//!
//! `big-media-components` factors out the GTK4/libadwaita widgets, dialogs,
//! and pure helpers shared by `big-video-player` and `big-audio-player`. It sits
//! *above* [`big_relm4_components`] and *below* the per-app crates.
//!
//! # Module groups
//!
//! - Pure formatting and parsing: [`bytes`], [`time`], [`track_labels`],
//!   [`playlist_order`].
//! - File and metadata helpers: [`media`], [`ffmpeg`].
//! - GTK widgets and dialogs: [`audio`], [`playback`], [`surface`],
//!   [`preference_rows`], [`initials_avatar`].
//! - Translation hook: [`text`] and [`install_media_components_translator`].
//!
//! # Translation
//!
//! Per-app translations are installed once at startup via
//! [`install_media_components_translator`]; widgets and dialogs use the
//! installed hook through [`text::text`].
//!
//! # Examples
//!
//! Format a duration and a byte count for a queue row:
//!
//! ```
//! use big_media_components::{bytes::decimal_compact, time::duration_hms};
//!
//! assert_eq!(decimal_compact(1_073_741_824), "1.0 GB");
//! assert_eq!(duration_hms(3661.0), "1:01:01");
//! ```

/// Audio labels and equalizer settings parsing.
pub mod audio;
/// Human-readable byte-size formatting.
pub mod bytes;
/// FFmpeg codec label helpers.
pub mod ffmpeg;
/// Initials-based avatar widgets and color helpers.
pub mod initials_avatar;
/// Generic media browsing helpers: file types, thumbnails, info grid.
pub mod media;
/// Playback controls: volume icons and the generic [`playback::queue::PlayQueue`].
pub mod playback;
/// Player-UI chrome shared across player front-ends: off-thread spawn, play
/// pulse, volume/speed/voice popovers (extracted from `big-media-runtime`).
pub mod player_chrome;
/// Playlist ordering: shuffle and stable path sort.
pub mod playlist_order;
/// libadwaita preference-row builders for player/converter settings.
pub mod preference_rows;
/// Full-window media surface, chrome, OSD layering, and immersion policy.
pub mod surface;
/// Process-local translation hook used by widgets in this crate.
pub mod text;
/// Media time/duration formatting for transports and tooltips.
pub mod time;
/// Track label formatting for mpv and ffprobe stream lists.
pub mod track_labels;

pub use text::install_media_components_translator;
