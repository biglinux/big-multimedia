//! Shared runtime for BigLinux multimedia applications.

/// Cast pipeline: GPU detection, ffprobe, transcoding, and optional Chromecast.
pub mod cast;
/// Compile-time configuration constants (paths, domain names).
/// Moved to the GTK-free `big-media-data` crate; re-exported for compat.
pub use big_media_data::config;
/// Batch video converter with GPU acceleration.
pub mod converter;
/// SQLite database layer for the music library (in `big-media-data`).
#[cfg(feature = "audio")]
pub use big_media_data::db;
/// Media editor UI components (video + audio), reusable across media apps.
#[cfg(feature = "video")]
pub mod editor;
/// Unified `Error` enum + crate-wide `Result<T>` alias (in `big-media-data`).
pub use big_media_data::error;
// Re-export at crate root so consumers can write
// `big_media_runtime::Error` / `big_media_runtime::Result<T>` instead of
// the longer `error::` path — matches industry convention for library
// crates and keeps consumer use-paths short.
pub use big_media_data::{Error, Result};
// browser_node (GObject) + download (manager) moved to `big-player-ui` (GTK
// reusable UI). Apps import them from there, not via the engine runtime.
/// Internationalization helper (`gettext` wrapper, in `big-media-data`).
pub use big_media_data::i18n;
/// Music library scanning, metadata, web clients (in `big-media-data`).
#[cfg(feature = "audio")]
pub use big_media_data::library;
/// Media utilities: playlist handling, metadata probing, thumbnails.
#[cfg(feature = "ui-core")]
pub mod media;
/// MPRIS D-Bus media player interface (active for audio and video players).
#[cfg(feature = "ui-core")]
pub mod mpris;
/// mpv backend, playback queue, and thumbnail generation.
#[cfg(feature = "ui-core")]
pub mod player;
// Reusable Relm4 player-UI panels moved to `big-player-ui` (engine-free crate).
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
/// Locale-aware UI label arrays shared by converter front-ends.
/// Ungated: it needs nothing but `i18n`, and `big-quick-export` depends on it
/// without wanting the GTK stack.
pub mod ui_labels;
