use super::*;
use std::path::Path;

#[test]
fn is_audio_file_mp3() {
    assert!(is_audio_file("/music/song.mp3"));
}

#[test]
fn is_audio_file_flac_uppercase() {
    assert!(is_audio_file("/music/song.FLAC"));
}

#[test]
fn is_audio_file_opus() {
    assert!(is_audio_file("track.opus"));
}

#[test]
fn is_audio_file_rejects_video() {
    assert!(!is_audio_file("movie.mkv"));
}

#[test]
fn is_audio_file_no_extension() {
    assert!(!is_audio_file("/path/noext"));
}

#[test]
fn is_audio_file_empty_path() {
    assert!(!is_audio_file(""));
}

#[test]
fn is_audio_file_all_known_extensions() {
    for ext in AUDIO_EXTENSIONS {
        assert!(
            is_audio_file(&format!("file.{ext}")),
            "expected true for .{ext}"
        );
    }
}

#[test]
fn is_video_file_mkv() {
    assert!(is_video_file("/video/movie.mkv"));
}

#[test]
fn is_video_file_mp4_uppercase() {
    assert!(is_video_file("/video/movie.MP4"));
}

#[test]
fn is_video_file_all_known_extensions() {
    for ext in VIDEO_EXTENSIONS {
        if *ext == "ts" {
            continue;
        }
        assert!(
            is_video_file(&format!("file.{ext}")),
            "expected true for .{ext}"
        );
    }
}

#[test]
fn is_video_file_accepts_mpeg_ts_stream() {
    let dir = tempfile::tempdir().unwrap();
    let ts = dir.path().join("stream.ts");
    std::fs::write(&ts, [0x47_u8; 564]).unwrap();
    assert!(is_video_file(ts.to_str().unwrap()));
}

#[test]
fn is_video_file_rejects_non_mpeg_ts_stream() {
    let dir = tempfile::tempdir().unwrap();
    let ts = dir.path().join("stream.ts");
    std::fs::write(&ts, [0_u8; 564]).unwrap();
    assert!(!is_video_file(ts.to_str().unwrap()));
}

#[test]
fn is_video_file_rejects_audio() {
    assert!(!is_video_file("song.mp3"));
}

#[test]
fn is_video_file_rejects_unknown() {
    assert!(!is_video_file("file.xyz"));
}

#[test]
fn is_media_file_audio() {
    assert!(is_media_file("track.ogg"));
}

#[test]
fn is_media_file_video() {
    assert!(is_media_file("clip.webm"));
}

#[test]
fn is_media_file_rejects_text() {
    assert!(!is_media_file("readme.txt"));
}

#[test]
fn is_playlist_m3u() {
    assert!(is_playlist_file("list.m3u"));
}

#[test]
fn is_playlist_m3u8() {
    assert!(is_playlist_file("/path/to/list.m3u8"));
}

#[test]
fn is_playlist_pls() {
    assert!(is_playlist_file("radio.pls"));
}

#[test]
fn is_playlist_uppercase() {
    assert!(is_playlist_file("LIST.M3U"));
}

#[test]
fn is_playlist_rejects_mp3() {
    assert!(!is_playlist_file("song.mp3"));
}

#[test]
fn is_code_project_dir_at_root() {
    assert!(!is_code_project_dir(Path::new("/")));
}

#[test]
fn scan_folder_flat_filters_media_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.mp3"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();
    std::fs::write(dir.path().join("c.mp4"), "").unwrap();
    let files = scan_folder_flat(dir.path());
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|file| file.ends_with("a.mp3")));
    assert!(files.iter().any(|file| file.ends_with("c.mp4")));
}

#[test]
fn list_subfolders_returns_visible_directories_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("albums")).unwrap();
    std::fs::create_dir(dir.path().join(".cache")).unwrap();
    let dirs = list_subfolders(dir.path());
    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].ends_with("albums"));
}
