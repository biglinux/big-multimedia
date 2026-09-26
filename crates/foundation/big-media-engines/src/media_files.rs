// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Media file type detection shared by players, converters, and scanners.
//!
//! Detection is extension-based for speed; the only file-content sniff is
//! [`is_mpeg_ts`](crate::media_files::is_mpeg_ts), used to disambiguate `.ts`
//! (TypeScript) from MPEG-TS.

use std::path::Path;

/// Container extensions recognised as video: those of the MIME types the
/// video player's desktop entry claims, so a file it is asked to open is also
/// one its folder scan lists.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "mk3d", "webm", "avi", "mov", "qt", "flv", "wmv", "mpg", "mpeg", "mpe", "m2v",
    "ts", "m2ts", "vob", "ogv", "ogm", "3gp", "3g2", "m4v", "mts", "f4v", "divx", "asf", "dv",
    "mxf", "rm", "rmvb", "nsv", "mj2", "mjpeg", "mjpg", "flc", "fli", "bik", "smk", "axv", "3gpp",
    "3gp2", "3gpp2", "mjp2", "m2t", "rv", "bk2", "lrv", "moov",
];

/// Container extensions recognised as audio, likewise the audio player's.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "wav", "m4a", "m4b", "aac", "wma", "ape", "wv", "aiff", "aif",
    "aifc", "mka", "oga", "spx", "ac3", "dts", "mpc", "tta", "amr", "awb", "au", "snd", "dff",
    "dsf", "weba", "mod", "it", "s3m", "xm", "mpga", "adts", "f4a", "f4b", "aiffc", "dtshd", "mpp",
    "loas",
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
    // Two sync bytes one packet apart: a single `G` (0x47) in a short
    // TypeScript file must not pass for a video stream.
    (0..read.min(188))
        .any(|start| start + 188 < read && buf[start] == 0x47 && buf[start + 188] == 0x47)
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
        let short_source = dir.join("tiny.ts");
        std::fs::write(&good, [0x47_u8; 564]).unwrap();
        std::fs::write(&bad, [0_u8; 564]).unwrap();
        std::fs::write(&short_source, b"export const Greeting = 'hi';\n").unwrap();

        assert!(is_video_file(good.to_str().unwrap()));
        assert!(!is_video_file(bad.to_str().unwrap()));
        assert!(!is_video_file(short_source.to_str().unwrap()));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn playlist_detection_covers_common_formats() {
        assert!(is_playlist_file("list.m3u"));
        assert!(is_playlist_file("list.M3U8"));
        assert!(is_playlist_file("radio.pls"));
        assert!(!is_playlist_file("song.mp3"));
    }
}
