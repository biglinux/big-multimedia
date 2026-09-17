//! Reusable Relm4 player-UI panels for BigLinux media players.
//!
//! Engine-free: depends only on `big-media-data` (data services), the component
//! crates, and `big-os-kit` — never on `big-media-runtime` (mpv/ffmpeg). Each
//! panel is a push-model `Component` (owns its widgets + emits typed intents;
//! the host shell owns the queue/mpv/db/settings and pushes display state back).
//! See `big-rust-components/ARCHITECTURE.md` §9–10.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

/// Appearance/color preference pages shared by the audio and video players.
pub mod appearance;
/// Shared audio preferences page plus live-player callback types.
pub mod audio_preferences;
/// Shared visual audio trim bar used by player transports.
#[cfg(feature = "audio-panels")]
pub mod audio_trim_bar;
/// `BrowserNode` GObject node type for the file/library browser tree.
#[cfg(feature = "audio-panels")]
pub mod browser_node;
/// Generic HTTP download-manager backend (worker + per-task state/widgets).
#[cfg(feature = "audio-panels")]
pub mod download;
/// Bounded favicon-texture LRU cache (`gdk::Texture`) for the radio panel.
pub mod favicon_cache;
/// Illustration/artwork rendering helpers.
pub mod illustration;

/// Shared cut timeline drawing and pointer interaction.
pub mod cut_timeline;
/// Slide-up download-manager panel.
#[cfg(feature = "audio-panels")]
pub mod download_panel;
/// Flat directory file browser.
#[cfg(feature = "audio-panels")]
pub mod file_browser;
/// Lyrics panel (lyrics / loading / empty stack).
#[cfg(feature = "audio-panels")]
pub mod lyrics_panel;
/// Virtualized media list (recycling `ListView`) shared by radio + podcast.
pub mod media_list;
/// Audio playback bar (transport + volume/speed/voice popovers).
#[cfg(feature = "audio-panels")]
pub mod playback_bar;
/// Podcast subscriptions + RSS search panel.
#[cfg(feature = "audio-panels")]
pub mod podcast_panel;
/// Radio tab: saved stations + Radio-Browser search/explore.
#[cfg(feature = "audio-panels")]
pub mod radio_panel;
/// Smartphone web-remote pairing dialog.
#[cfg(feature = "audio-panels")]
pub mod smartphone;
