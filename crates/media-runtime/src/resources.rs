//! Compiled GTK resources consumed by split application crates.

pub const VIDEO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/video.gresource"));
pub const AUDIO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/audio.gresource"));
