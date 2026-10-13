//! Public facade for playlist scanning, parsing, and metadata. The heavy
//! lifting lives in `fuzzy_search`, `metadata_cache`, and `playlist_scan`.

pub use super::fuzzy_search::fuzzy_match;
pub use super::metadata_cache::{
    DetailedVideoMetadata, fast_duration_cached, meta_summary, probe_detailed_video_metadata,
    probe_metadata, probe_metadata_cached, resolution_label,
};
pub use super::playlist_scan::{
    IptvChannel, breadcrumb_segments, export_m3u, folder_display_name, format_file_size,
    is_audio_file, is_media_file, is_playlist_file, is_video_file, list_subfolders, parse_m3u,
    parse_playlist_file, restore_from_trash, scan_folder_flat, scan_folder_recursive,
    shuffled_order, sort_paths, trash_file, write_m3u,
};
pub use big_media_components::media::files::{AUDIO_EXTENSIONS, VIDEO_EXTENSIONS};

#[derive(Default)]
pub struct ScanResult {
    pub files: Vec<String>,
    pub truncated: bool,
    pub total_found: usize,
}

pub const SCAN_LIMIT: usize = 5000;

#[derive(Clone, Debug, PartialEq)]
pub enum EntryKind {
    ParentFolder,
    Folder,
    Video,
}

#[derive(Clone, Debug)]
pub struct PlaylistEntry {
    pub path: String,
    pub kind: EntryKind,
    pub index: usize,
    pub display_name: String,
}

#[derive(Clone, Default)]
pub struct VideoMeta {
    pub resolution: Option<String>,
    pub codec: Option<String>,
    pub duration_secs: Option<f64>,
    pub audio_tracks: u32,
    pub subtitle_tracks: u32,
}

/// Filename without its directory and extension.
#[must_use]
pub fn display_name(path: &str) -> String {
    big_app_kit::files::file_stem_display_name(path)
}
