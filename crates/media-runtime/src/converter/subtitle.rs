//! Subtitle helpers: ffprobe stream listing, external file matching by stem
//! (.srt/.ass/.ssa/.sub/.vtt), single-track extraction, multi-segment merge
//! with timecode rewriting for ffmpeg join mode. Bitmap codecs (PGS/VOBSUB/
//! DVB) skipped by the merge path — they can't be re-timed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use log::warn;

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

pub fn extract_subtitle(input: &Path, stream_index: u32) -> Option<PathBuf> {
    let subtitle_temp_dir = std::env::temp_dir().join("big-video-converter");
    std::fs::create_dir_all(&subtitle_temp_dir).ok()?;

    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("subtitle");
    let out_path = subtitle_temp_dir.join(format!("{stem}_track{stream_index}.srt"));

    let status = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args(["-y", "-i"])
        .arg(input)
        .args(["-map", &format!("0:{stream_index}"), "-c:s", "srt"])
        .arg("--")
        .arg(&out_path)
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(30))
        .build()
        .run();

    match status {
        Ok(o) if o.status.success() => Some(out_path),
        Ok(o) => {
            warn!(
                "Subtitle extraction failed with exit code {}",
                o.status.code().unwrap_or(-1)
            );
            None
        }
        Err(e) => {
            warn!("Subtitle extraction error: {e}");
            None
        }
    }
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
pub fn build_joined_subtitle_files(
    input: &Path,
    segments: &[crate::converter::ffmpeg::TrimRange],
    temp_dir: &Path,
) -> Vec<(PathBuf, String)> {
    use std::io::Write;

    let streams = probe_subtitles(input);
    let text_streams: Vec<_> = streams.iter().filter(|s| !s.is_bitmap).collect();
    if text_streams.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();

    for stream in &text_streams {
        let full_srt = temp_dir.join(format!("sub_{}_full.srt", stream.index));
        let ok = BigSubprocessSpec::builder()
            .program("ffmpeg")
            .args(["-y", "-i"])
            .arg(input)
            .args(["-map", &format!("0:{}", stream.index), "-c:s", "srt"])
            .arg("--")
            .arg(&full_srt)
            .stdout(BigSubprocessOutputMode::Null)
            .stderr(BigSubprocessOutputMode::Null)
            .timeout(Duration::from_secs(30))
            .build()
            .run()
            .is_ok_and(|o| o.status.success());
        if !ok || !full_srt.exists() {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(&full_srt) else {
            continue;
        };

        let mut merged = Vec::new();
        let mut cumulative = 0.0_f64;
        let mut seq = 1_u32;

        for seg in segments {
            let seg_start = seg.start.unwrap_or(0.0);
            let seg_end = seg.end.unwrap_or(f64::MAX);

            for entry in parse_srt_entries(&content) {
                if entry.start >= seg_start && entry.end <= seg_end {
                    let new_start = entry.start - seg_start + cumulative;
                    let new_end = entry.end - seg_start + cumulative;
                    merged.push(format!(
                        "{seq}\n{} --> {}\n{}",
                        secs_to_srt_timecode(new_start),
                        secs_to_srt_timecode(new_end),
                        entry.text
                    ));
                    seq += 1;
                }
            }
            cumulative += seg_end - seg_start;
        }

        if merged.is_empty() {
            continue;
        }

        let lang = if stream.language.is_empty() {
            "und"
        } else {
            &stream.language
        };
        let out_path = temp_dir.join(format!("merged_{lang}.srt"));
        if let Ok(mut f) = std::fs::File::create(&out_path) {
            let _ = f.write_all(merged.join("\n\n").as_bytes());
            results.push((out_path, lang.to_string()));
        }
    }

    results
}

struct SrtEntry {
    start: f64,
    end: f64,
    text: String,
}

fn parse_srt_entries(content: &str) -> Vec<SrtEntry> {
    let mut entries = Vec::new();
    let blocks: Vec<&str> = content.split("\n\n").collect();
    for block in blocks {
        let lines: Vec<&str> = block.trim().lines().collect();
        if lines.len() >= 3
            && let Some((start_tc, end_tc)) = lines[1].split_once(" --> ")
        {
            let start = srt_timecode_to_secs(start_tc.trim());
            let end = srt_timecode_to_secs(end_tc.trim());
            let text = lines[2..].join("\n");
            entries.push(SrtEntry { start, end, text });
        }
    }
    entries
}

fn srt_timecode_to_secs(tc: &str) -> f64 {
    // SRT format: HH:MM:SS,mmm
    let (time_part, ms_str) = tc.split_once(',').unwrap_or((tc, "0"));
    let parts: Vec<&str> = time_part.split(':').collect();
    if parts.len() != 3 {
        return 0.0;
    }
    let h: f64 = parts[0].parse().unwrap_or(0.0);
    let m: f64 = parts[1].parse().unwrap_or(0.0);
    let s: f64 = parts[2].parse().unwrap_or(0.0);
    let ms: f64 = ms_str.parse().unwrap_or(0.0);
    h * 3600.0 + m * 60.0 + s + ms / 1000.0
}

fn secs_to_srt_timecode(secs: f64) -> String {
    let h = (secs / 3600.0) as u32;
    let m = ((secs % 3600.0) / 60.0) as u32;
    let s = (secs % 60.0) as u32;
    let ms = ((secs % 1.0) * 1000.0) as u32;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

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

    #[test]
    fn srt_timecode_to_secs_sums_all_components() {
        // 1h + 2m + 3s + 500ms, every component distinct and non-zero so the
        // whole h*3600 + m*60 + s + ms/1000 chain is pinned.
        assert!((srt_timecode_to_secs("01:02:03,500") - 3723.5).abs() < 1e-9);
        assert!((srt_timecode_to_secs("00:00:00,000")).abs() < 1e-9);
        // A missing milliseconds field defaults to 0 (comma split fallback).
        assert!((srt_timecode_to_secs("00:00:05") - 5.0).abs() < 1e-9);
        // Malformed (not three colon parts) → 0 (pins the `!= 3` guard).
        assert!((srt_timecode_to_secs("01:02")).abs() < 1e-9);
    }

    #[test]
    fn secs_to_srt_timecode_formats_hh_mm_ss_ms() {
        assert_eq!(secs_to_srt_timecode(3723.5), "01:02:03,500");
        assert_eq!(secs_to_srt_timecode(0.0), "00:00:00,000");
        // Round-trips with the parser.
        assert_eq!(
            secs_to_srt_timecode(srt_timecode_to_secs("00:10:09,250")),
            "00:10:09,250"
        );
    }

    #[test]
    fn parse_srt_entries_reads_blocks() {
        let content = "1\n00:00:01,000 --> 00:00:02,500\nHello world\n\n\
                       2\n00:00:03,000 --> 00:00:04,000\nLine A\nLine B";
        let entries = parse_srt_entries(content);
        assert_eq!(entries.len(), 2);
        assert!((entries[0].start - 1.0).abs() < 1e-9);
        assert!((entries[0].end - 2.5).abs() < 1e-9);
        assert_eq!(entries[0].text, "Hello world");
        // Multi-line cue text is preserved (lines[2..] joined).
        assert_eq!(entries[1].text, "Line A\nLine B");
        // A block with fewer than 3 lines is ignored (pins `len >= 3`).
        assert!(parse_srt_entries("1\n00:00:01,000 --> 00:00:02,000").is_empty());
    }

    #[test]
    fn is_bitmap_subtitle_codec_recognises_only_bitmap_formats() {
        assert!(is_bitmap_subtitle_codec("dvd_subtitle"));
        assert!(is_bitmap_subtitle_codec("hdmv_pgs_subtitle"));
        assert!(is_bitmap_subtitle_codec("vobsub"));
        assert!(!is_bitmap_subtitle_codec("subrip"));
        assert!(!is_bitmap_subtitle_codec("ass"));
        assert!(!is_bitmap_subtitle_codec(""));
    }
}
