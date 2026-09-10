//! GTK-free data services shared by BigLinux media apps.
//!
//! Extracted from `big-media-runtime` so the reusable player UI (`big-player-ui`)
//! and lean consumers can use the SQLite library, settings store, and network
//! clients without linking mpv/ffmpeg engines. No GTK in this crate — see
//! `big-rust-components/ARCHITECTURE.md` §9–10 for the placement rule. SQLite
//! uses the system libsqlite3 and is feature-gated behind `db`.
//!
//! HTTP + URL helpers come from `big_os_kit` (GTK-free leaf, single policy home).

#![forbid(unsafe_code)]

/// Compile-time configuration constants (paths, domain names).
pub mod config;
/// SQLite database layer for the music library.
#[cfg(feature = "db")]
pub mod db;
/// Unified `Error` enum + crate-wide `Result<T>` alias.
pub mod error;
/// Internationalization helper (`gettext` wrapper).
pub mod i18n;
/// Music library scanning, metadata reading, and web-metadata clients
/// (MusicBrainz, Last.fm, ListenBrainz, LRCLIB, cover art, podcasts).
#[cfg(feature = "library")]
pub mod library;
/// Radio data layer: Radio-Browser API client (GTK-free).
#[cfg(feature = "radio")]
pub mod radio;
/// Persistent key/value settings store + typed key catalog.
pub mod settings;

// Re-export at the crate root so consumers can write `big_media_data::Error` /
// `big_media_data::Result<T>` — matches the convention `big-media-runtime` uses
// (and which the moved `db` queries rely on via `crate::Result`).
pub use error::{Error, Result};
