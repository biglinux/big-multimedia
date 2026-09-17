//! Public facade for playlist scanning, parsing, and metadata. The heavy
//! lifting lives in `fuzzy_search`, `metadata_cache`, and `playlist_scan`.

pub use super::fuzzy_search::fuzzy_match;
pub use super::metadata_cache::{
    DetailedVideoMetadata, fast_duration_cached, meta_summary, probe_detailed_video_metadata,
    probe_metadata, probe_metadata_cached, resolution_label,
};
pub use super::playlist_scan::{
    IptvChannel, breadcrumb_segments, export_m3u, folder_display_name, format_file_size,
    is_audio_file, is_media_file, is_playlist_file, is_video_file, list_subfolders, parse_iptv_m3u,
    parse_m3u, parse_playlist_file, restore_from_trash, scan_folder_flat, scan_folder_recursive,
    shuffled_order, sort_paths, trash_file, write_m3u,
};
pub use big_media_components::media::files::{AUDIO_EXTENSIONS, VIDEO_EXTENSIONS};

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

/// Filename stem — strips path and extension.
#[must_use]
pub fn display_name(path: &str) -> String {
    big_app_kit::files::file_stem_display_name(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::playlist_scan::is_code_project_dir;
    use std::path::Path;

    // ── display_name ──

    #[test]
    fn display_name_extracts_stem() {
        assert_eq!(display_name("/home/user/video.mp4"), "video");
    }

    #[test]
    fn display_name_returns_stem_without_path() {
        assert_eq!(display_name("video.mp4"), "video");
    }

    #[test]
    fn display_name_handles_trailing_slash() {
        // Path::new strips trailing slash on file_name()
        assert_eq!(display_name("/home/user/folder"), "folder");
    }

    // ── is_video_file ──

    #[test]
    fn is_video_file_accepts_supported_video_extensions() {
        assert!(is_video_file("movie.mp4"));
        assert!(is_video_file("movie.mkv"));
        assert!(is_video_file("movie.avi"));
        assert!(is_video_file("movie.webm"));
    }

    #[test]
    fn is_video_file_case_insensitive() {
        assert!(is_video_file("movie.MP4"));
        assert!(is_video_file("movie.MKV"));
    }

    #[test]
    fn is_video_file_rejects_non_video() {
        assert!(!is_video_file("document.txt"));
        assert!(!is_video_file("image.png"));
        assert!(!is_video_file("script.py"));
        assert!(!is_video_file("noextension"));
    }

    // ── format_file_size ──

    #[test]
    fn format_file_size_bytes() {
        assert_eq!(format_file_size(500), "500 B");
    }

    #[test]
    fn format_file_size_kilobytes() {
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1536), "1.5 KB");
    }

    #[test]
    fn format_file_size_megabytes() {
        assert_eq!(format_file_size(1_048_576), "1.00 MB");
    }

    #[test]
    fn format_file_size_gigabytes() {
        assert_eq!(format_file_size(1_073_741_824), "1.00 GB");
    }

    // ── shuffled_order ──

    #[test]
    fn shuffled_order_preserves_count() {
        let order = shuffled_order(10, 0);
        assert_eq!(order.len(), 10);
    }

    #[test]
    fn shuffled_order_starts_with_current() {
        let order = shuffled_order(10, 5);
        assert_eq!(order[0], 5);
    }

    #[test]
    fn shuffled_order_contains_all_indices() {
        let order = shuffled_order(10, 3);
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn shuffled_order_keeps_single_index() {
        let order = shuffled_order(1, 0);
        assert_eq!(order, vec![0]);
    }

    // ── is_code_project_dir ──

    #[test]
    fn is_code_project_dir_at_root() {
        // Root ("/") is never a code project dir
        assert!(!is_code_project_dir(Path::new("/")));
    }

    // ── sort_paths ──

    #[test]
    fn sort_paths_by_name() {
        let mut paths = vec![
            "/videos/charlie.mp4".into(),
            "/videos/alice.mp4".into(),
            "/videos/bob.mp4".into(),
        ];
        sort_paths(&mut paths, "name", false);
        assert_eq!(
            paths,
            vec![
                "/videos/alice.mp4",
                "/videos/bob.mp4",
                "/videos/charlie.mp4",
            ]
        );
    }

    #[test]
    fn sort_paths_by_name_descending() {
        let mut paths = vec![
            "/videos/alice.mp4".into(),
            "/videos/charlie.mp4".into(),
            "/videos/bob.mp4".into(),
        ];
        sort_paths(&mut paths, "name", true);
        assert_eq!(
            paths,
            vec![
                "/videos/charlie.mp4",
                "/videos/bob.mp4",
                "/videos/alice.mp4",
            ]
        );
    }

    // ── is_audio_file ──

    #[test]
    fn is_audio_file_accepts_supported_audio_extensions() {
        assert!(is_audio_file("song.mp3"));
        assert!(is_audio_file("song.flac"));
        assert!(is_audio_file("song.ogg"));
        assert!(is_audio_file("song.opus"));
        assert!(is_audio_file("song.m4a"));
        assert!(is_audio_file("song.wav"));
    }

    #[test]
    fn is_audio_file_case_insensitive() {
        assert!(is_audio_file("song.MP3"));
        assert!(is_audio_file("song.FLAC"));
    }

    #[test]
    fn is_audio_file_rejects_non_audio() {
        assert!(!is_audio_file("movie.mp4"));
        assert!(!is_audio_file("document.txt"));
        assert!(!is_audio_file("noextension"));
    }

    // ── is_media_file ──

    #[test]
    fn is_media_file_accepts_video_and_audio() {
        assert!(is_media_file("movie.mp4"));
        assert!(is_media_file("song.flac"));
        assert!(is_media_file("clip.mkv"));
        assert!(is_media_file("track.opus"));
    }

    #[test]
    fn is_media_file_rejects_other() {
        assert!(!is_media_file("readme.txt"));
        assert!(!is_media_file("photo.png"));
    }

    // ── M3U round-trip ──

    #[test]
    fn m3u_write_and_parse_roundtrip() {
        let dir = std::env::temp_dir().join("bmp_test_m3u_roundtrip");
        let _ = std::fs::create_dir_all(&dir);

        // Create dummy files so parse_m3u's exists() check passes
        let files: Vec<String> = (0..3)
            .map(|i| {
                let p = dir.join(format!("track{i}.mp3"));
                std::fs::write(&p, b"fake").unwrap();
                p.to_string_lossy().to_string()
            })
            .collect();

        let m3u_path = dir.join("test.m3u8");
        write_m3u(&m3u_path, &files).unwrap();

        let parsed = parse_m3u(&m3u_path);
        assert_eq!(parsed, files);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn m3u_parse_skips_comments_and_empty_lines() {
        let dir = std::env::temp_dir().join("bmp_test_m3u_comments");
        let _ = std::fs::create_dir_all(&dir);

        let track = dir.join("song.flac");
        std::fs::write(&track, b"fake").unwrap();

        let m3u_path = dir.join("test.m3u");
        let content = format!(
            "#EXTM3U\n#EXTINF:120,Song Title\n{}\n\n# a comment\n",
            track.to_string_lossy()
        );
        std::fs::write(&m3u_path, &content).unwrap();

        let parsed = parse_m3u(&m3u_path);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], track.to_string_lossy());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── PLS round-trip ──

    #[test]
    fn pls_parse_extracts_file_entries() {
        let dir = std::env::temp_dir().join("bmp_test_pls_parse");
        let _ = std::fs::create_dir_all(&dir);

        let track = dir.join("song.ogg");
        std::fs::write(&track, b"fake").unwrap();

        let pls_path = dir.join("test.pls");
        let content = format!(
            "[playlist]\nFile1={}\nTitle1=Song\nLength1=120\nNumberOfEntries=1\nVersion=2\n",
            track.to_string_lossy()
        );
        std::fs::write(&pls_path, &content).unwrap();

        let parsed = parse_playlist_file(&pls_path.to_string_lossy());
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], track.to_string_lossy());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn m3u_parse_handles_urls() {
        let dir = std::env::temp_dir().join("bmp_test_m3u_urls");
        let _ = std::fs::create_dir_all(&dir);

        let m3u_path = dir.join("radio.m3u");
        let content = "#EXTM3U\n#EXTINF:-1,Radio\nhttps://stream.example.com/live\n";
        std::fs::write(&m3u_path, content).unwrap();

        let parsed = parse_m3u(&m3u_path);
        assert_eq!(parsed, vec!["https://stream.example.com/live"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_m3u_roundtrip() {
        let dir = std::env::temp_dir().join("bmp_test_export_roundtrip");
        let _ = std::fs::create_dir_all(&dir);

        let files: Vec<String> = (0..2)
            .map(|i| {
                let p = dir.join(format!("file{i}.opus"));
                std::fs::write(&p, b"fake").unwrap();
                p.to_string_lossy().to_string()
            })
            .collect();

        let out = dir.join("exported.m3u8");
        export_m3u(&files, &out.to_string_lossy()).unwrap();

        let parsed = parse_m3u(&out);
        assert_eq!(parsed, files);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
