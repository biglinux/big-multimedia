//! Subtitle helpers: ffprobe stream listing, external file matching by stem
//! (.srt/.ass/.ssa/.sub/.vtt), single-track extraction, multi-segment merge
//! with timecode rewriting for ffmpeg join mode. Bitmap codecs (PGS/VOBSUB/
//! DVB) skipped by the merge path — they can't be re-timed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

const TEXT_SUBTITLE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "sub", "vtt"];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubtitleInput {
    None,
    EmbeddedCopy,
    BurnTrack(u32),
    ExternalFile(PathBuf),
}

#[derive(Debug, Clone)]
pub struct SubtitleStream {
    pub index: u32,
    pub codec: String,
    pub language: String,
    pub title: String,
    pub is_bitmap: bool,
}

pub fn probe_subtitles(path: &Path) -> Vec<SubtitleStream> {
    let output = build_probe_subtitles_command(path).run();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let Some(streams) = json.get("streams").and_then(|s| s.as_array()) else {
        return Vec::new();
    };

    streams
        .iter()
        .filter_map(|s| {
            let index = s.get("index")?.as_u64()? as u32;
            let codec = s
                .get("codec_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tags = s.get("tags");
            let language = tags
                .and_then(|t| t.get("language"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = tags
                .and_then(|t| t.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let is_bitmap = is_bitmap_subtitle_codec(&codec);
            Some(SubtitleStream {
                index,
                codec,
                language,
                title,
                is_bitmap,
            })
        })
        .collect()
}

pub fn find_external_subtitles(video_path: &Path) -> Vec<PathBuf> {
    let Some(dir) = video_path.parent() else {
        return Vec::new();
    };
    let Some(stem) = video_path.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };

    let mut found = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if TEXT_SUBTITLE_EXTENSIONS.contains(&ext.as_str())
                && let Some(file_stem) = path.file_stem().and_then(|s| s.to_str())
            {
                // Matches "video.srt", "video.en.srt", "video_en.srt".
                if file_stem.starts_with(stem) {
                    found.push(path);
                }
            }
        }
    }

    found.sort();
    found
}

// Per-language SRT: filter entries to each segment range, rewrite timecodes
// against cumulative segment offset. Skipped for bitmap codecs.

// `--` terminator scopes path as positional — defends against hostile
// filenames beginning with `-` that ffprobe would parse as a flag.
fn build_probe_subtitles_command(path: &Path) -> BigSubprocessSpec {
    BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "quiet",
            "-select_streams",
            "s",
            "-show_entries",
            "stream=index,codec_name:stream_tags=language,title",
            "-of",
            "json",
            "--",
        ])
        .arg(path)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(10))
        .build()
}

fn is_bitmap_subtitle_codec(codec: &str) -> bool {
    matches!(
        codec,
        "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "xsub" | "pgssub" | "vobsub"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn probe_subtitles_command_separates_hostile_path() {
        let hostile = PathBuf::from("-f lavfi -i /dev/zero");
        let cmd = build_probe_subtitles_command(&hostile);
        let resolved = cmd.resolved();
        let mut argv = vec![resolved.program];
        argv.extend(resolved.argv);
        let target = hostile.to_string_lossy().into_owned();
        let idx = argv.iter().position(|a| a == &target).expect("path argv");
        assert!(idx > 0);
        let prev = &argv[idx - 1];
        assert!(prev == "--" || prev == "-i", "argv={argv:?}");
    }

    #[test]
    fn subtitle_input_none_debug() {
        let s = SubtitleInput::None;
        assert!(format!("{s:?}").contains("None"));
    }

    #[test]
    fn subtitle_input_external_file() {
        let s = SubtitleInput::ExternalFile(PathBuf::from("/tmp/subs.srt"));
        assert!(
            matches!(&s, SubtitleInput::ExternalFile(p) if p == &PathBuf::from("/tmp/subs.srt")),
            "unexpected variant: {s:?}",
        );
    }

    #[test]
    fn subtitle_input_burn_track() {
        let s = SubtitleInput::BurnTrack(2);
        assert!(
            matches!(s, SubtitleInput::BurnTrack(2)),
            "unexpected variant: {s:?}",
        );
    }

    #[test]
    fn subtitle_stream_bitmap_detection() {
        let stream = SubtitleStream {
            index: 0,
            codec: "hdmv_pgs_subtitle".into(),
            language: "eng".into(),
            title: String::new(),
            is_bitmap: true,
        };
        assert!(stream.is_bitmap);

        let text_stream = SubtitleStream {
            index: 1,
            codec: "subrip".into(),
            language: "por".into(),
            title: "Portuguese".into(),
            is_bitmap: false,
        };
        assert!(!text_stream.is_bitmap);
    }

    #[test]
    fn find_external_subtitles_matches_same_stem() {
        let subtitle_test_dir = std::env::temp_dir().join("test_ext_subs");
        let _ = fs::create_dir_all(&subtitle_test_dir);

        let video = subtitle_test_dir.join("movie.mp4");
        fs::write(&video, "fake").unwrap();
        fs::write(subtitle_test_dir.join("movie.srt"), "sub1").unwrap();
        fs::write(subtitle_test_dir.join("movie.en.srt"), "sub2").unwrap();
        fs::write(subtitle_test_dir.join("movie.ass"), "sub3").unwrap();
        fs::write(subtitle_test_dir.join("other.srt"), "sub4").unwrap();

        let found = find_external_subtitles(&video);
        assert!(found.len() >= 3, "expected >=3 matches, got: {found:?}");
        assert!(found.iter().any(|p| p.ends_with("movie.srt")));
        assert!(found.iter().any(|p| p.ends_with("movie.en.srt")));
        assert!(found.iter().any(|p| p.ends_with("movie.ass")));
        assert!(!found.iter().any(|p| p.ends_with("other.srt")));

        let _ = fs::remove_dir_all(&subtitle_test_dir);
    }

    #[test]
    fn find_external_subtitles_empty_dir() {
        let subtitle_test_dir = std::env::temp_dir().join("test_ext_subs_empty");
        let _ = fs::create_dir_all(&subtitle_test_dir);
        let video = subtitle_test_dir.join("video.mp4");
        fs::write(&video, "fake").unwrap();

        let found = find_external_subtitles(&video);
        assert!(found.is_empty());

        let _ = fs::remove_dir_all(&subtitle_test_dir);
    }

    #[test]
    fn find_external_subtitles_no_parent() {
        let video = PathBuf::from("orphan.mp4");
        // CWD may or may not have matches; just verify no panic.
        let found = find_external_subtitles(&video);
        let _ = found;
    }

    #[test]
    fn find_external_subtitles_sorted() {
        let subtitle_test_dir = std::env::temp_dir().join("test_ext_subs_sort");
        let _ = fs::create_dir_all(&subtitle_test_dir);
        let video = subtitle_test_dir.join("clip.mp4");
        fs::write(&video, "fake").unwrap();
        fs::write(subtitle_test_dir.join("clip.vtt"), "vtt").unwrap();
        fs::write(subtitle_test_dir.join("clip.srt"), "srt").unwrap();
        fs::write(subtitle_test_dir.join("clip.ass"), "ass").unwrap();

        let found = find_external_subtitles(&video);
        let is_sorted = found.windows(2).all(|w| w[0] <= w[1]);
        assert!(is_sorted, "results should be sorted, got: {found:?}");

        let _ = fs::remove_dir_all(&subtitle_test_dir);
    }

    #[test]
    fn find_external_subtitles_sub_extension() {
        let subtitle_test_dir = std::env::temp_dir().join("test_ext_subs_sub");
        let _ = fs::create_dir_all(&subtitle_test_dir);
        let video = subtitle_test_dir.join("movie.mp4");
        fs::write(&video, "fake").unwrap();
        fs::write(subtitle_test_dir.join("movie.sub"), "sub content").unwrap();
        fs::write(subtitle_test_dir.join("movie.ssa"), "ssa content").unwrap();

        let found = find_external_subtitles(&video);
        assert!(found.iter().any(|p| p.ends_with("movie.sub")));
        assert!(found.iter().any(|p| p.ends_with("movie.ssa")));

        let _ = fs::remove_dir_all(&subtitle_test_dir);
    }
}
