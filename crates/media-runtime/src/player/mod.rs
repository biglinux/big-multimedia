//! Player subsystem — mpv backend integration, playback queue management,
//! and video thumbnail extraction.

pub mod mpv_backend;
#[cfg(feature = "audio")]
pub mod queue;
pub mod thumbnail;
