//! IPTV playlist modeling and parsing.
//!
//! One `IptvChannel` model and one M3U parser serve every binary, including
//! the plain playlist reader in `playlist_scan`.

mod channel;
mod m3u;
mod provider;

pub use channel::IptvChannel;
pub use m3u::{
    MAX_PLAYLIST_BYTES, decode_playlist, parse_m3u_content, parse_m3u_file, read_playlist_text,
};
pub use provider::{IptvKind, IptvProvider, IptvProviderStore};
