//! File validation: extension routing into Video / AudioOnly / Unsupported
//! using the `runtime_config` extension sets, plus ffmpeg-availability probe
//! and a human-readable size formatter shared by file_metadata_dialog and
//! progress live updates.

use std::path::Path;
use std::time::Duration;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

use super::runtime_config;

#[derive(Debug)]
pub enum FileCheck {
    Video,
    AudioOnly,
    Unsupported(String),
}

pub fn validate_file(path: &Path) -> FileCheck {
    if !path.exists() {
        return FileCheck::Unsupported("File not found".into());
    }

    if !path.is_file() {
        return FileCheck::Unsupported("Not a file".into());
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let runtime_config = runtime_config::config();
    if runtime_config.has_video_ext(&ext) {
        FileCheck::Video
    } else if runtime_config.has_audio_ext(&ext) {
        FileCheck::AudioOnly
    } else {
        FileCheck::Unsupported(format!("Unsupported extension: .{ext}"))
    }
}

pub fn ffmpeg_available() -> bool {
    BigSubprocessSpec::builder()
        .program("ffmpeg")
        .arg("-version")
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .is_ok_and(|o| o.status.success())
}

pub fn format_size(bytes: u64) -> String {
    decimal_compact(bytes)
}

fn decimal_compact(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if bytes >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn video_exts() -> HashSet<String> {
        runtime_config::config().video_extensions.clone()
    }

    fn audio_exts() -> HashSet<String> {
        runtime_config::config().audio_extensions.clone()
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1024), "1 KB");
        assert_eq!(format_size(1_048_576), "1.0 MB");
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
    }

    #[test]
    fn test_format_size_intermediate() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(5_242_880), "5.0 MB");
        assert_eq!(format_size(2_147_483_648), "2.0 GB");
    }

    #[test]
    fn test_video_extensions() {
        let v = video_exts();
        assert!(v.contains("mp4"));
        assert!(v.contains("mkv"));
        assert!(v.contains("webm"));
        assert!(v.contains("avi"));
        assert!(v.contains("mov"));
        assert!(v.contains("ts"));
        assert!(v.contains("m2ts"));
        assert!(!v.contains("mp3"));
        assert!(!v.contains("txt"));
    }

    #[test]
    fn test_audio_extensions() {
        let a = audio_exts();
        assert!(a.contains("mp3"));
        assert!(a.contains("flac"));
        assert!(a.contains("wav"));
        assert!(a.contains("opus"));
        assert!(a.contains("ogg"));
        assert!(a.contains("m4a"));
        assert!(!a.contains("mp4"));
        assert!(!a.contains("txt"));
    }

    // Pin the shipping video set; refactor must update this list intentionally.
    #[test]
    fn characterization_video_ext_set() {
        let expected: HashSet<String> = [
            "mp4", "mkv", "webm", "avi", "mov", "wmv", "flv", "ts", "m2ts", "mts", "m4v", "mpg",
            "mpeg", "3gp", "ogv", "vob", "f4v", "divx", "asf",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(video_exts(), expected);
        assert_eq!(video_exts().len(), 19);
    }

    // Pin the shipping audio set.
    #[test]
    fn characterization_audio_ext_set() {
        let expected: HashSet<String> = [
            "mp3", "flac", "wav", "aac", "ogg", "opus", "wma", "m4a", "aiff", "ape", "wv", "ac3",
            "dts", "mka", "aif", "oga",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(audio_exts(), expected);
        assert_eq!(audio_exts().len(), 16);
    }

    #[test]
    fn characterization_validate_every_video_ext() {
        for ext in video_exts() {
            let path = std::env::temp_dir().join(format!("char_video.{ext}"));
            std::fs::write(&path, b"x").unwrap();
            let result = validate_file(&path);
            let _ = std::fs::remove_file(&path);
            assert!(
                matches!(result, FileCheck::Video),
                ".{ext} should be Video, got {result:?}"
            );
        }
    }

    #[test]
    fn characterization_validate_every_audio_ext() {
        for ext in audio_exts() {
            let path = std::env::temp_dir().join(format!("char_audio.{ext}"));
            std::fs::write(&path, b"x").unwrap();
            let result = validate_file(&path);
            let _ = std::fs::remove_file(&path);
            assert!(
                matches!(result, FileCheck::AudioOnly),
                ".{ext} should be AudioOnly, got {result:?}"
            );
        }
    }

    #[test]
    fn characterization_case_insensitive() {
        let path = std::env::temp_dir().join("char_case.MP4");
        std::fs::write(&path, b"x").unwrap();
        let result = validate_file(&path);
        let _ = std::fs::remove_file(&path);
        assert!(matches!(result, FileCheck::Video));
    }

    #[test]
    fn test_validate_file_nonexistent() {
        let path = PathBuf::from("/nonexistent/file.mp4");
        let result = validate_file(&path);
        let FileCheck::Unsupported(ref msg) = result else {
            panic!("expected Unsupported, got {result:?}");
        };
        assert!(msg.contains("not found"), "msg: {msg}");
    }

    #[test]
    fn test_validate_file_directory() {
        let path = PathBuf::from("/tmp");
        let result = validate_file(&path);
        let FileCheck::Unsupported(ref msg) = result else {
            panic!("expected Unsupported, got {result:?}");
        };
        assert!(msg.contains("Not a file"), "msg: {msg}");
    }

    #[test]
    fn test_validate_file_unsupported_ext() {
        let path = std::env::temp_dir().join("test_validator.txt");
        std::fs::write(&path, "test").unwrap();
        let result = validate_file(&path);
        let _ = std::fs::remove_file(&path);
        let FileCheck::Unsupported(ref msg) = result else {
            panic!("expected Unsupported, got {result:?}");
        };
        assert!(msg.contains("Unsupported extension"), "msg: {msg}");
    }

    #[test]
    fn test_validate_file_video() {
        let path = std::env::temp_dir().join("test_validator.mp4");
        std::fs::write(&path, "fake video").unwrap();
        let result = validate_file(&path);
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(result, FileCheck::Video),
            "unexpected variant: {result:?}"
        );
    }

    #[test]
    fn test_validate_file_audio() {
        let path = std::env::temp_dir().join("test_validator.flac");
        std::fs::write(&path, "fake audio").unwrap();
        let result = validate_file(&path);
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(result, FileCheck::AudioOnly),
            "unexpected variant: {result:?}"
        );
    }
}
