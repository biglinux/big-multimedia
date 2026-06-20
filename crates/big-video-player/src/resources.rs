//! App-owned GTK resources.

pub const VIDEO_PLAYER_SHELL: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/video-player-shell.gresource"));
