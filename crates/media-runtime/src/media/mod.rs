//! Media abstraction: playlist parsing, metadata caching, fuzzy search,
//! duration helpers, and playlist scan utilities.

pub mod fuzzy_search;
/// IPTV M3U parsing and channel modeling shared by all binaries.
pub mod iptv;
pub mod metadata_cache;
pub(crate) mod native_duration;
#[cfg(fuzzing)]
pub use native_duration::fuzz_read_duration;
pub mod playlist;
/// Playlist file save picker and result alert.
pub mod playlist_export;
pub mod playlist_scan;
pub mod thumbnail;
