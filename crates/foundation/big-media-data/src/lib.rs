//! GTK-free data services shared by BigLinux media apps.
//!
//! The reusable player UI (`big-player-ui`) and lean consumers use the SQLite
//! library, settings store, and network clients without linking mpv/ffmpeg
//! engines. No GTK in this crate. SQLite
//! uses the system libsqlite3 and is feature-gated behind `db`.
//!
//! HTTP + URL helpers come from `big_os_kit` (GTK-free leaf, single policy home).
//! [`media_probe`] runs ffprobe for every crate that reads a media file's
//! streams and tags.
//!
//! - [`config`]: compile-time configuration constants (paths, domain names).
//! - `db`: SQLite database layer for the music library.
//! - [`error`]: unified `Error` enum + crate-wide `Result<T>` alias.
//! - [`i18n`]: internationalization helper (`gettext` wrapper).
//! - `library`: music library scanning, metadata reading, and web-metadata clients
//!   (MusicBrainz, LRCLIB, cover art, podcasts).
//! - [`media_probe`]: ffprobe's JSON report on a media file, shared by the audio
//!   library and the video metadata cache.
//! - `radio`: radio data layer: Radio-Browser API client (GTK-free).
//! - [`settings`]: persistent key/value settings store + typed key catalog.

#![forbid(unsafe_code)]

pub mod config;
#[cfg(feature = "db")]
pub mod db;
pub mod error;
pub mod i18n;
#[cfg(feature = "library")]
pub mod library;
pub mod media_probe;
#[cfg(feature = "radio")]
pub mod radio;
pub mod settings;

// Root re-export, as in `big-media-runtime`; the `db` queries use
// `crate::Result`.
pub use error::{Error, Result};
