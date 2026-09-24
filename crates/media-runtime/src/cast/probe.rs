//! Detect whether a media file is Chromecast-compatible using ffprobe.

use std::path::Path;

use big_app_kit::subprocess::BigSubprocessSpec;
use log::warn;
use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct CastProbeResult {
    pub direct_compatible: bool,
    pub video_codec: String,
    pub audio_codec: String,
    pub audio_needs_transcode: bool,
    pub video_needs_transcode: bool,
    /// `0.0` when ffprobe couldn't determine duration.
    pub duration: f64,
    pub pixel_format: String,
    /// H.264 10-bit fails on Chromecast — flagged so caller can force transcode.
    pub is_10bit: bool,
    pub width: u32,
    pub height: u32,
}

fn run_ffprobe(file_path: &str) -> Result<String> {
    let args = ffprobe_streams_argv(file_path, true);
    let Some(sandboxed) =
        crate::cast::sandbox::wrap_args("ffprobe", &args, &[Path::new(file_path)], &[])
    else {
        return Err(Error::ffmpeg(
            "bwrap",
            None,
            "bubblewrap sandbox unavailable — refusing to probe untrusted media",
        ));
    };
    let output = BigSubprocessSpec::builder()
        .program("bwrap")
        .args(sandboxed)
        .build()
        .run()
        .map_err(|e| Error::ffmpeg("ffprobe", None, format!("spawn failed: {e}")))?;

    if !output.status.success() {
        return Err(Error::ffmpeg(
            "ffprobe",
            output.status.code(),
            "non-zero exit",
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// ffprobe argv for stream (and optionally format) info. `--` terminates flag
/// parsing so a hostile filename beginning with `-` is read as a positional
/// path, not an option — kept separate from the sandbox wrapping so it stays
/// unit-testable.
fn ffprobe_streams_argv(file_path: &str, show_format: bool) -> Vec<&str> {
    let mut argv = vec!["-v", "quiet", "-print_format", "json", "-show_streams"];
    if show_format {
        argv.push("-show_format");
    }
    argv.push("--");
    argv.push(file_path);
    argv
}

pub fn probe_cast_compatibility(file_path: &str) -> Result<CastProbeResult> {
    let json_str = run_ffprobe(file_path)?;
    Ok(parse_ffprobe_json(&json_str))
}

fn get_str(ffprobe_stream: &Value, key: &str) -> String {
    ffprobe_stream
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// First video + first audio stream wins. Empty-codec result on parse failure
/// or no streams (caller logs warn).
pub(crate) fn parse_ffprobe_json(json: &str) -> CastProbeResult {
    let root: Value = serde_json::from_str(json).unwrap_or(Value::Null);

    let mut video_codec = String::new();
    let mut audio_codec = String::new();
    let mut pixel_format = String::new();
    let mut width = 0_u32;
    let mut height = 0_u32;

    if let Some(streams) = root.get("streams").and_then(Value::as_array) {
        for stream in streams {
            match get_str(stream, "codec_type").as_str() {
                "video" if video_codec.is_empty() => {
                    video_codec = get_str(stream, "codec_name");
                    pixel_format = get_str(stream, "pix_fmt");
                    width = stream.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
                    height = stream.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
                }
                "audio" if audio_codec.is_empty() => {
                    audio_codec = get_str(stream, "codec_name");
                }
                _ => {}
            }
        }
    }

    // Duration lives in `format.duration` (string) or on individual streams.
    let duration = root
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    let is_10bit = pixel_format.contains("10le")
        || pixel_format.contains("10be")
        || pixel_format.contains("p10");

    let video_needs_transcode = !video_codec.is_empty()
        && (!big_media_components::container_compat::is_chromecast_video_codec(&video_codec)
            || (video_codec == "h264" && is_10bit));

    let audio_needs_transcode = !audio_codec.is_empty()
        && !big_media_components::container_compat::is_chromecast_audio_codec(&audio_codec);

    let direct_compatible = !video_needs_transcode && !audio_needs_transcode;

    if video_codec.is_empty() && audio_codec.is_empty() {
        warn!("ffprobe found no video or audio streams");
    }

    CastProbeResult {
        direct_compatible,
        video_codec,
        audio_codec,
        audio_needs_transcode,
        video_needs_transcode,
        duration,
        pixel_format,
        is_10bit,
        width,
        height,
    }
}

#[derive(Debug, Clone)]
pub struct MediaTrack {
    pub stream_index: usize,
    pub title: String,
    pub language: String,
    pub codec: String,
}

impl MediaTrack {
    pub fn label(&self, sequential: usize) -> String {
        big_media_components::track_labels::ffprobe_stream_label(
            sequential,
            &self.title,
            &self.language,
            &self.codec,
            "Track",
        )
    }

    /// Bitmap subs (PGS/VOBSUB/DVB) can't be re-rendered to canvas — caller must
    /// burn-in or skip; text subs (SRT/ASS) don't need this.
    pub fn is_bitmap_subtitle(&self) -> bool {
        big_media_components::subtitle::codecs::is_bitmap_subtitle_codec(&self.codec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_track_label_with_title() {
        let t = MediaTrack {
            stream_index: 0,
            title: "English".into(),
            language: "eng".into(),
            codec: "aac".into(),
        };
        assert_eq!(t.label(1), "1: English (aac)");
    }

    #[test]
    fn media_track_label_language_fallback() {
        let t = MediaTrack {
            stream_index: 0,
            title: String::new(),
            language: "por".into(),
            codec: "ac3".into(),
        };
        assert_eq!(t.label(2), "2: por (ac3)");
    }

    #[test]
    fn media_track_label_generic_fallback() {
        let t = MediaTrack {
            stream_index: 0,
            title: String::new(),
            language: String::new(),
            codec: "opus".into(),
        };
        assert_eq!(t.label(3), "3: Track 3 (opus)");
    }

    #[test]
    fn is_bitmap_subtitle_pgs() {
        let t = MediaTrack {
            stream_index: 0,
            title: String::new(),
            language: String::new(),
            codec: "hdmv_pgs_subtitle".into(),
        };
        assert!(t.is_bitmap_subtitle());
    }

    #[test]
    fn is_bitmap_subtitle_text() {
        let t = MediaTrack {
            stream_index: 0,
            title: String::new(),
            language: String::new(),
            codec: "subrip".into(),
        };
        assert!(!t.is_bitmap_subtitle());
    }

    #[test]
    fn parse_ffprobe_h264_aac_compatible() {
        let json = r#"{
    "streams": [
        {
            "codec_type": "video",
            "codec_name": "h264",
            "pix_fmt": "yuv420p",
            "width": 1920,
            "height": 1080
        },
        {
            "codec_type": "audio",
            "codec_name": "aac"
        }
    ],
    "format": {
        "duration": "120.500"
    }
}"#;
        let r = parse_ffprobe_json(json);
        assert!(r.direct_compatible);
        assert!(!r.video_needs_transcode);
        assert!(!r.audio_needs_transcode);
        assert_eq!(r.video_codec, "h264");
        assert_eq!(r.audio_codec, "aac");
        assert_eq!(r.width, 1920);
        assert_eq!(r.height, 1080);
        assert!(!r.is_10bit);
        assert!((r.duration - 120.5).abs() < 0.001);
    }

    #[test]
    fn parse_ffprobe_h264_10bit_needs_transcode() {
        let json = r#"{
    "streams": [
        {
            "codec_type": "video",
            "codec_name": "h264",
            "pix_fmt": "yuv420p10le",
            "width": 1920,
            "height": 1080
        },
        {
            "codec_type": "audio",
            "codec_name": "aac"
        }
    ]
}"#;
        let r = parse_ffprobe_json(json);
        assert!(r.is_10bit);
        assert!(r.video_needs_transcode);
        assert!(!r.direct_compatible);
    }

    #[test]
    fn parse_ffprobe_unsupported_audio_needs_transcode() {
        let json = r#"{
    "streams": [
        {
            "codec_type": "video",
            "codec_name": "h264",
            "pix_fmt": "yuv420p",
            "width": 1280,
            "height": 720
        },
        {
            "codec_type": "audio",
            "codec_name": "ac3"
        }
    ]
}"#;
        let r = parse_ffprobe_json(json);
        assert!(r.audio_needs_transcode);
        assert!(!r.video_needs_transcode);
        assert!(!r.direct_compatible);
    }

    #[test]
    fn parse_ffprobe_vp9_opus_compatible() {
        let json = r#"{
    "streams": [
        {
            "codec_type": "video",
            "codec_name": "vp9",
            "pix_fmt": "yuv420p",
            "width": 3840,
            "height": 2160
        },
        {
            "codec_type": "audio",
            "codec_name": "opus"
        }
    ]
}"#;
        let r = parse_ffprobe_json(json);
        assert!(r.direct_compatible);
        assert_eq!(r.video_codec, "vp9");
        assert_eq!(r.audio_codec, "opus");
    }

    #[test]
    fn parse_ffprobe_garbage_returns_empty() {
        let r = parse_ffprobe_json("not json at all");
        assert!(r.video_codec.is_empty());
        assert!(r.audio_codec.is_empty());
    }

    // Regression: hostile filename starting with `-` must be preceded by
    // `--` so ffprobe parses it as a positional file, not a flag.
    #[test]
    fn ffprobe_streams_argv_separates_hostile_path() {
        for show_format in [true, false] {
            let hostile = "-f lavfi -i /dev/zero";
            let argv = ffprobe_streams_argv(hostile, show_format);
            let idx = argv
                .iter()
                .position(|a| *a == hostile)
                .expect("path in argv");
            assert!(idx > 0, "path must not be argv[0]");
            assert_eq!(
                argv[idx - 1],
                "--",
                "expected `--` before hostile path; argv={argv:?}",
            );
        }
    }

    #[test]
    fn parse_keeps_first_video_and_audio_stream() {
        // Two video and two audio streams: the FIRST of each wins (pins the
        // `*_codec.is_empty()` match guards).
        let json = r#"{
    "streams": [
        { "codec_type": "video", "codec_name": "h264", "pix_fmt": "yuv420p" },
        { "codec_type": "video", "codec_name": "hevc", "pix_fmt": "yuv420p" },
        { "codec_type": "audio", "codec_name": "aac" },
        { "codec_type": "audio", "codec_name": "ac3" }
    ]
}"#;
        let r = parse_ffprobe_json(json);
        assert_eq!(r.video_codec, "h264", "first video wins");
        assert_eq!(r.audio_codec, "aac", "first audio wins");
    }

    #[test]
    fn is_10bit_detects_each_pixel_format_token() {
        let probe = |pix: &str| {
            let json = format!(
                r#"{{"streams":[{{"codec_type":"video","codec_name":"hevc","pix_fmt":"{pix}"}}]}}"#
            );
            parse_ffprobe_json(&json).is_10bit
        };
        // Each disjunct token alone must flag 10-bit (pins the `||` chain).
        assert!(probe("zz10le"), "10le token");
        assert!(probe("zz10be"), "10be token");
        assert!(probe("zzp10zz"), "p10 token");
        // An 8-bit format is not 10-bit.
        assert!(!probe("yuv420p"));
    }

    #[test]
    fn video_only_stream_stays_compatible() {
        // No audio stream: audio must NOT be flagged as needing transcode
        // (pins the `!audio_codec.is_empty() && ...` conjunction). A
        // Chromecast-friendly video alone is directly compatible.
        let json = r#"{
    "streams": [
        { "codec_type": "video", "codec_name": "h264", "pix_fmt": "yuv420p" }
    ]
}"#;
        let r = parse_ffprobe_json(json);
        assert!(r.audio_codec.is_empty());
        assert!(!r.audio_needs_transcode, "absent audio needs no transcode");
        assert!(r.direct_compatible, "compatible video with no audio");
    }
}
