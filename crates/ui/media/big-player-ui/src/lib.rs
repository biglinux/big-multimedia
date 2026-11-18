//! Reusable Relm4 player-UI panels for BigLinux media players.
//!
//! Engine-free: depends only on `big-media-data` (data services), the component
//! crates, and `big-os-kit`, never on `big-media-runtime` (mpv/ffmpeg). Each
//! panel is a push-model `Component` (owns its widgets + emits typed intents;
//! the host shell owns the queue/mpv/db/settings and pushes display state back).
//!
//! - [`appearance`]: appearance/color preference pages shared by the audio and video
//!   players.
//! - [`audio_preferences`]: shared audio preferences page plus live-player callback
//!   types.
//! - `audio_trim_bar`: shared visual audio trim bar used by player transports.
//! - `browser_node`: `BrowserNode` GObject node type for the file/library browser tree.
//! - [`cut_timeline`]: shared cut timeline drawing and pointer interaction.
//! - `download`: generic HTTP download-manager backend (worker + per-task
//!   state/widgets).
//! - `download_panel`: slide-up download-manager panel.
//! - [`favicon_cache`]: bounded favicon-texture LRU cache (`gdk::Texture`) for the
//!   radio panel.
//! - `file_browser`: flat directory file browser.
//! - `lyrics_panel`: lyrics panel (lyrics / loading / empty stack).
//! - [`media_list`]: virtualized media list (recycling `ListView`) shared by radio +
//!   podcast.
//! - `playback_bar`: audio playback bar (transport + volume/speed/voice popovers).
//! - `podcast_panel`: podcast subscriptions + RSS search panel.
//! - [`preference_rows`]: preference rows bound to a typed settings key, shared by both
//!   players.
//! - `radio_panel`: radio tab: saved stations + Radio-Browser search/explore.
//! - `smartphone`: smartphone web-remote pairing dialog.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod appearance;
pub mod audio_preferences;
#[cfg(feature = "audio-panels")]
pub mod audio_trim_bar;
#[cfg(feature = "audio-panels")]
pub mod browser_node;
pub mod cut_timeline;
#[cfg(feature = "audio-panels")]
pub mod download;
#[cfg(feature = "audio-panels")]
pub mod download_panel;
pub mod favicon_cache;
#[cfg(feature = "audio-panels")]
pub mod file_browser;
#[cfg(feature = "audio-panels")]
pub mod lyrics_panel;
pub mod media_list;
#[cfg(feature = "audio-panels")]
pub mod playback_bar;
#[cfg(feature = "audio-panels")]
pub mod podcast_panel;
pub mod preference_rows;
#[cfg(feature = "audio-panels")]
pub mod radio_panel;
#[cfg(feature = "audio-panels")]
pub mod smartphone;
