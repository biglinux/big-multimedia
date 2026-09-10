// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Media file type detection shared by players, converters, and scanners.
//!
//! Detection is extension-based for speed; the only file-content sniff is
//! [`is_mpeg_ts`](crate::media_files::is_mpeg_ts), used to disambiguate `.ts`
//! (TypeScript) from MPEG-TS.

use std::path::Path;

/// Container extensions recognised as video.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "webm", "avi", "mov", "flv", "wmv", "mpg", "mpeg", "ts", "m2ts", "vob", "ogv",
    "3gp", "m4v", "mts", "f4v", "divx", "asf",
];

/// Container extensions recognised as audio.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "wav", "m4a", "aac", "wma", "ape", "wv", "aiff", "aif", "mka",
    "oga", "ac3", "dts",
];

/// Playlist file extensions.
pub const PLAYLIST_EXTENSIONS: &[&str] = &["m3u", "m3u8", "pls"];

/// `true` when `path` has an audio extension from [`AUDIO_EXTENSIONS`].
#[must_use]
pub fn is_audio_file(path: &str) -> bool {
    extension(path).is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.as_str()))
}

/// `true` when `path` has a video extension and is not a TypeScript file mis-claimed as `.ts`.
#[must_use]
pub fn is_video_file(path: &str) -> bool {
    let Some(ext) = extension(path) else {
        return false;
    };
    if !VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        return false;
    }
    if ext == "ts" {
        return is_mpeg_ts(path);
    }
    true
}

/// `true` when `path` is either an audio or a video file.
#[must_use]
pub fn is_media_file(path: &str) -> bool {
    is_video_file(path) || is_audio_file(path)
}

/// `true` when `path` has a playlist extension from [`PLAYLIST_EXTENSIONS`].
#[must_use]
pub fn is_playlist_file(path: &str) -> bool {
    extension(path).is_some_and(|ext| PLAYLIST_EXTENSIONS.contains(&ext.as_str()))
}

/// Sniff `path` for the MPEG-TS `0x47` sync byte pattern.
///
/// MPEG transport streams emit a `0x47` sync byte every 188 bytes. Reading
/// up to 564 bytes is enough to catch the first three sync points.
#[must_use]
pub fn is_mpeg_ts(path: &str) -> bool {
    use std::io::Read;

    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0_u8; 564];
    let read = file.read(&mut buf).unwrap_or(0);
    if read == 0 {
        return false;
    }
    for start in 0..read.min(188) {
        if buf[start] == 0x47 && (start + 188 >= read || buf[start + 188] == 0x47) {
            return true;
        }
    }
    false
}

/// Index of the first non-audio entry in `paths`, or [`None`] if there is none.
#[must_use]
pub fn first_video_index(paths: &[String]) -> Option<usize> {
    paths.iter().position(|path| !is_audio_file(path))
}

/// Index of the next non-audio entry after `pos`, skipping audio rows.
#[must_use]
pub fn next_video_index(paths: &[String], pos: i64) -> Option<usize> {
    let mut next = pos + 1;
    while (next as usize) < paths.len() && is_audio_file(&paths[next as usize]) {
        next += 1;
    }
    ((next as usize) < paths.len()).then_some(next as usize)
}

/// Index of the previous non-audio entry before `pos`, skipping audio rows.
#[must_use]
pub fn previous_video_index(paths: &[String], pos: i64) -> Option<usize> {
    if pos <= 0 {
        return None;
    }
    let mut prev = pos - 1;
    while prev > 0 && is_audio_file(&paths[prev as usize]) {
        prev -= 1;
    }
    (prev >= 0 && !is_audio_file(&paths[prev as usize])).then_some(prev as usize)
}

/// `true` when `path` is an HTTP(S)/RTSP/RTMP URL rather than a local file.
#[must_use]
pub fn is_stream_path(path: &str) -> bool {
    path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("rtsp://")
        || path.starts_with("rtmp://")
}

fn extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_detection_is_case_insensitive() {
        assert!(is_audio_file("/music/song.FLAC"));
        assert!(is_audio_file("/music/surround.dts"));
        assert!(!is_audio_file("/video/movie.mkv"));
    }

    #[test]
    fn video_detection_sniffs_ts() {
        let dir = std::env::temp_dir().join(format!("big-media-ts-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let good = dir.join("stream.ts");
        let bad = dir.join("script.ts");
        std::fs::write(&good, [0x47_u8; 564]).unwrap();
        std::fs::write(&bad, [0_u8; 564]).unwrap();

        assert!(is_video_file(good.to_str().unwrap()));
        assert!(!is_video_file(bad.to_str().unwrap()));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn playlist_detection_covers_common_formats() {
        assert!(is_playlist_file("list.m3u"));
        assert!(is_playlist_file("list.M3U8"));
        assert!(is_playlist_file("radio.pls"));
        assert!(!is_playlist_file("song.mp3"));
    }

    #[test]
    fn video_index_helpers_skip_audio_entries() {
        let paths = vec![
            "/tmp/a.mp3".to_string(),
            "/tmp/v1.mp4".to_string(),
            "/tmp/b.flac".to_string(),
            "/tmp/v2.mkv".to_string(),
        ];

        assert_eq!(first_video_index(&paths), Some(1));
        assert_eq!(next_video_index(&paths, 1), Some(3));
        assert_eq!(next_video_index(&paths, 3), None);
        assert_eq!(previous_video_index(&paths, 3), Some(1));
        assert_eq!(previous_video_index(&paths, 1), None);
    }

    #[test]
    fn stream_detection_accepts_common_protocols() {
        assert!(is_stream_path("https://example.test/live.m3u8"));
        assert!(is_stream_path("rtsp://camera/live"));
        assert!(is_stream_path("rtmp://server/live"));
        assert!(!is_stream_path("/home/user/video.mp4"));
    }
}
