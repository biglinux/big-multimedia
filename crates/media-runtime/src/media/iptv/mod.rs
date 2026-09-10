//! IPTV playlist modeling and parsing.
//!
//! One canonical `IptvChannel` struct and one `parse_m3u` function serve
//! every binary. Historically these lived in two places (`video/iptv.rs`
//! and `playlist_scan/playlist_io.rs`) with subtly different field sets;
//! the duplication is gone.

mod channel;
mod m3u;
mod provider;

pub use channel::IptvChannel;
pub use m3u::{parse_m3u_content, parse_m3u_file};
pub use provider::{IptvKind, IptvProvider, IptvProviderStore};
