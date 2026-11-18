//! Compiled GTK resources consumed by split application crates.

pub const AUDIO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/audio.gresource"));
