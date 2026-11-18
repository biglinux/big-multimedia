//! IPTV playlist modeling and parsing.
//!
//! One `IptvChannel` model and one M3U parser serve every binary, including
//! the plain playlist reader in `playlist_scan`.

mod channel;
mod m3u;
mod provider;

pub use channel::IptvChannel;
pub use m3u::{
    MAX_PLAYLIST_BYTES, MAX_PLAYLIST_ENTRIES, PlaylistEntries, decode_playlist, left_out_notice,
    parse_channel_list, parse_m3u_content, read_playlist_text,
};
pub use provider::{IptvKind, IptvProvider, IptvProviderStore};
