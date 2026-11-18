//! Folder scanning, playlist I/O (M3U/PLS/IPTV), sort, trash operations.

mod display;
mod playlist_io;
mod scan;
#[cfg(test)]
mod tests;
mod trash;

pub use crate::media::iptv::IptvChannel;
pub use big_media_components::media::files::{is_audio_file, is_media_file, is_video_file};
pub use big_media_components::playlist_order::sort_paths;
pub use display::{breadcrumb_segments, folder_display_name};
pub use playlist_io::{is_playlist_file, parse_m3u, parse_playlist_file, write_m3u};
pub use scan::{list_subfolders, scan_folder_flat, scan_folder_recursive};
pub use trash::{restore_from_trash, trash_file};
