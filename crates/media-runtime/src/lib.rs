//! Shared runtime for BigLinux multimedia applications.

/// Cast pipeline: GPU detection, ffprobe, transcoding, and optional Chromecast.
pub mod cast;
/// Compile-time configuration constants (paths, domain names), from the
/// GTK-free `big-media-data` crate.
pub use big_media_data::config;
/// SQLite database layer for the music library (in `big-media-data`).
#[cfg(feature = "audio")]
pub use big_media_data::db;
/// Unified `Error` enum + crate-wide `Result<T>` alias (in `big-media-data`).
pub use big_media_data::error;
// Re-export at crate root so consumers can write
// `big_media_runtime::Error` / `big_media_runtime::Result<T>` instead of
// the longer `error::` path — matches industry convention for library
// crates and keeps consumer use-paths short.
/// Internationalization helper (`gettext` wrapper, in `big-media-data`).
pub use big_media_data::i18n;
/// Music library scanning, metadata, web clients (in `big-media-data`).
#[cfg(feature = "audio")]
pub use big_media_data::library;
pub use big_media_data::{Error, Result};
/// Media utilities: playlist handling, metadata probing, thumbnails.
#[cfg(feature = "ui-core")]
pub mod media;
/// mpv backend, playback queue, and thumbnail generation.
#[cfg(feature = "ui-core")]
pub mod player;
/// Recently-played file tracking.
#[cfg(feature = "ui-core")]
pub mod recent;
/// Compiled GTK resources shared by split application crates.
#[cfg(feature = "ui-core")]
pub mod resources;
/// Persistent key/value settings store.
#[cfg(feature = "ui-core")]
pub mod settings;
/// Code shared between audio and video players.
#[cfg(feature = "ui-core")]
pub mod shared;
