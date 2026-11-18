//! Shared runtime for BigLinux multimedia applications.

/// Client of the BigNetScreen Chromecast/DLNA daemon.
pub mod cast;
/// Compile-time configuration constants (paths, domain names), from the
/// GTK-free `big-media-data` crate.
pub use big_media_data::config;
/// SQLite database layer for the music library (in `big-media-data`).
#[cfg(feature = "audio")]
pub use big_media_data::db;
/// Unified `Error` enum + crate-wide `Result<T>` alias (in `big-media-data`).
pub use big_media_data::error;
/// Internationalization helper (`gettext` wrapper, in `big-media-data`).
pub use big_media_data::i18n;
/// Music library scanning, metadata, web clients (in `big-media-data`).
#[cfg(feature = "audio")]
pub use big_media_data::library;
/// ffprobe reports and the protocols ffmpeg may open (in `big-media-data`).
pub use big_media_data::media_probe;
// Root paths for consumers: `big_media_runtime::Error` / `Result<T>`.
pub use big_media_data::{Error, Result};
/// Process boot (env, gettext, locale, resources), the About dialog and the
/// slot that delivers opened files to the open shell.
#[cfg(feature = "ui-core")]
pub mod app;
/// Audio effects: equalizer, noise reduction, and filter chain management.
#[cfg(feature = "ui-core")]
pub mod audio_effects;
/// Hand files (and marked cuts) to the external converter GUIs.
#[cfg(feature = "ui-core")]
pub mod converter_handoff;
/// Off-thread library database queries.
#[cfg(feature = "audio")]
pub mod db_query;
/// Media utilities: playlist handling, metadata probing, thumbnails.
#[cfg(feature = "ui-core")]
pub mod media;
/// mpv backend, playback queue, and seek-bar preview frames.
#[cfg(feature = "ui-core")]
pub mod player;
/// Recently-played file tracking.
#[cfg(feature = "ui-core")]
pub mod recent;
/// Compiled GTK resources shared by split application crates and the
/// illustrations built from them.
#[cfg(feature = "ui-core")]
pub mod resources;
/// Persistent key/value settings store.
#[cfg(feature = "ui-core")]
pub mod settings;
/// Shortcut capture dialog.
#[cfg(feature = "ui-core")]
pub mod shortcut_capture;
/// Sleep timer labels, actions, settings persistence and countdown.
#[cfg(feature = "ui-core")]
pub mod sleep_timer;
/// Color parsing and Adwaita CSS override generation for custom themes.
#[cfg(feature = "ui-core")]
pub mod theming;
