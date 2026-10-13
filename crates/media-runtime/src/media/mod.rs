//! Media abstraction: playlist parsing, metadata caching, fuzzy search,
//! duration helpers, and playlist scan utilities.

pub mod fuzzy_search;
/// IPTV M3U parsing and channel modeling shared by all binaries.
pub mod iptv;
pub mod metadata_cache;
pub(crate) mod native_duration;
pub mod playlist;
pub mod playlist_scan;
pub mod thumbnail;
