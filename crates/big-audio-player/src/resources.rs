//! App-owned GTK resources.

pub const AUDIO_PLAYER_SHELL: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/audio-player-shell.gresource"));
