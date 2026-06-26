//! Shared converter app UI and app-owned window flow.
//!
//! Submodules:
//!
//! - [`app`] — `adw::Application` subclass plus the global
//!   [`app::ConverterMode`] selector.
//! - [`settings`] — flat-key Settings wrapper and save/restore helpers
//!   for audio-effect sidebar widgets.
//! - [`ui`] — the Relm4 component tree (window, header, sidebar,
//!   queue, edit pages, dialogs).
//!
//! Re-exports from `big_media_runtime::converter` are kept as targeted
//! aliases so this crate can refer to runtime FFmpeg / queue / profile
//! types via `crate::converter::<name>` without taking on a full
//! upstream wildcard.

pub mod app;
pub mod daemon;
pub mod settings;
pub mod ui;

pub use big_media_runtime::converter::{ffmpeg, file_validator, profiles, queue, subtitle};
