//! Shared runtime for BigLinux multimedia applications.
//!
//! - `app`: process boot (env, gettext, locale, resources), the About dialog
//!   and the slot that delivers opened files to the open shell.
//! - `audio_effects`: equalizer, noise reduction and the voice strength slider
//!   bound to mpv and settings.
//! - `cast`: client of the BigNetScreen Chromecast/DLNA daemon and the "Play on
//!   a TV" page and card.
//! - `converter_handoff`: hands files (and marked cuts) to the converter GUIs.
//! - `db_query`: off-thread library database queries.
//! - `media`: playlists, metadata probing, IPTV lists and thumbnails.
//! - `player`: mpv backend, playback queue and seek-bar preview frames.
//! - `recent`: recently played files.
//! - `resources`: compiled GTK resources and the illustrations built from them.
//! - `settings`: persistent key/value settings store.
//! - `shortcut_capture`: shortcut capture dialog.
//! - `sleep_timer`: sleep timer labels, actions, persistence and countdown.
//! - `theming`: color parsing and Adwaita CSS overrides for custom themes.

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
#[cfg(feature = "ui-core")]
pub mod app;
#[cfg(feature = "ui-core")]
pub mod audio_effects;
#[cfg(feature = "ui-core")]
pub mod converter_handoff;
#[cfg(feature = "audio")]
pub mod db_query;
#[cfg(feature = "ui-core")]
pub mod media;
#[cfg(feature = "ui-core")]
pub mod player;
#[cfg(feature = "ui-core")]
pub mod recent;
#[cfg(feature = "ui-core")]
pub mod resources;
#[cfg(feature = "ui-core")]
pub mod settings;
#[cfg(feature = "ui-core")]
pub mod shortcut_capture;
#[cfg(feature = "ui-core")]
pub mod sleep_timer;
#[cfg(feature = "ui-core")]
pub mod theming;
