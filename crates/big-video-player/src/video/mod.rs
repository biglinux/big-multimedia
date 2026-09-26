//! Video player UI — mpv-backed video playback with DMA-BUF rendering,
//! Chromecast support, subtitle management, and appearance customisation.
//! The UI lives in [`ui`]; `keybindings` and `redaction` are engine-free
//! helpers reused by the Relm4 components.

pub mod app;
pub mod keybindings;
pub mod redaction;
/// App-local Relm4 glue for the video player (the live UI).
pub mod ui;
