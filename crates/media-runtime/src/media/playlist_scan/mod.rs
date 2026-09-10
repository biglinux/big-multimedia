//! Folder scanning, playlist I/O (M3U/PLS/IPTV), shuffle, sort, trash operations.

mod detect;
mod display;
mod ordering;
mod playlist_io;
mod scan;
#[cfg(test)]
mod tests;

pub use crate::media::iptv::IptvChannel;
pub use big_media_components::bytes::decimal_detailed as format_file_size;
#[cfg(test)]
pub(crate) use detect::is_code_project_dir;
pub use detect::{is_audio_file, is_media_file, is_video_file};
pub use display::{breadcrumb_segments, folder_display_name, restore_from_trash, trash_file};
pub use ordering::{shuffled_order, sort_paths};
pub use playlist_io::{
    export_m3u, is_playlist_file, parse_iptv_m3u, parse_m3u, parse_playlist_file, write_m3u,
};
pub use scan::{list_subfolders, scan_folder_flat, scan_folder_recursive};
