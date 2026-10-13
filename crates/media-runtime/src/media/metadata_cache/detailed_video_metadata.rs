use std::path::Path;

use big_app_kit::subprocess::BigSubprocessSpec;
use serde_json::Value;

use crate::i18n::i18n;
use crate::media::playlist::format_file_size;

use super::probe::codec_display_name;

/// Streams grouped by type; each is a `(label, value)` list ready for display.
pub struct DetailedVideoMetadata {
    pub file_name: String,
    pub file_dir: String,
    pub file_size: Option<String>,
    pub duration: Option<String>,
    pub format_name: Option<String>,
    pub bitrate: Option<String>,
    pub video_streams: Vec<Vec<(String, String)>>,
    pub audio_streams: Vec<Vec<(String, String)>>,
    pub subtitle_streams: Vec<Vec<(String, String)>>,
}

/// Synchronous ffprobe invocation; call from a background thread.
pub fn probe_detailed_video_metadata(path: &str) -> Option<DetailedVideoMetadata> {
    let output = BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
            // `--` defends against filenames starting with `-`.
            "--",
            path,
        ])
        .build()
        .run()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json = String::from_utf8_lossy(&output.stdout);
    let root: Value = serde_json::from_str(&json).ok()?;

    let p = Path::new(path);
    let file_name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let file_dir = p
        .parent()
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_default();

    let file_size = std::fs::metadata(path)
        .ok()
        .map(|m| format_file_size(m.len()));
    let format = &root["format"];
    let duration = format["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .map(ffmpeg_timestamp);
    let format_name = format["format_long_name"].as_str().map(String::from);
    let bitrate = format["bit_rate"].as_str().and_then(|b| {
        b.parse::<f64>()
            .ok()
            .map(|br| format!("{:.2} kbps", br / 1000.0))
    });

    let mut video_streams = Vec::new();
    let mut audio_streams = Vec::new();
    let mut subtitle_streams = Vec::new();

    if let Some(streams) = root["streams"].as_array() {
        for stream in streams {
            match stream["codec_type"].as_str() {
                Some("video") => video_streams.push(parse_video_stream(stream)),
                Some("audio") => audio_streams.push(parse_audio_stream(stream)),
                Some("subtitle") => subtitle_streams.push(parse_subtitle_stream(stream)),
                _ => {}
            }
        }
    }

    Some(DetailedVideoMetadata {
        file_name,
        file_dir,
        file_size,
        duration,
        format_name,
        bitrate,
        video_streams,
        audio_streams,
        subtitle_streams,
    })
}

pub(super) fn parse_video_stream(stream: &Value) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    if let Some(cn) = stream["codec_name"].as_str() {
        let mut codec = codec_display_name(cn);
        if let Some(profile) = stream["profile"].as_str() {
            codec = format!("{codec} ({profile})");
        }
        fields.push((i18n("Codec"), codec));
    }
    if let (Some(w), Some(h)) = (stream["width"].as_i64(), stream["height"].as_i64()) {
        fields.push((i18n("Resolution"), format!("{w}×{h}")));
    }
    if let Some(fps) = stream["r_frame_rate"].as_str()
        && let Some((num, den)) = fps.split_once('/')
        && let (Ok(n), Ok(d)) = (num.parse::<f64>(), den.parse::<f64>())
        && d > 0.0
    {
        fields.push((i18n("Frame Rate"), format!("{:.3} fps", n / d)));
    }
    if let Some(pf) = stream["pix_fmt"].as_str() {
        fields.push((i18n("Pixel Format"), pf.to_string()));
    }
    if let Some(br) = stream["bit_rate"].as_str()
        && let Ok(brf) = br.parse::<f64>()
    {
        fields.push((i18n("Bitrate"), format!("{:.2} kbps", brf / 1000.0)));
    }
    if let Some(lang) = stream["tags"]["language"].as_str() {
        fields.push((i18n("Language"), lang.to_uppercase()));
    }
    fields
}

pub(super) fn parse_audio_stream(stream: &Value) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    if let Some(cn) = stream["codec_name"].as_str() {
        let mut codec = cn.to_uppercase();
        if let Some(profile) = stream["profile"].as_str() {
            codec = format!("{codec} ({profile})");
        }
        fields.push((i18n("Codec"), codec));
    }
    if let Some(sr) = stream["sample_rate"].as_str()
        && let Ok(rate) = sr.parse::<u64>()
    {
        let quality = if rate == 44100 {
            Some(i18n("CD Quality"))
        } else if rate >= 48000 {
            Some(i18n("Hi-Res"))
        } else {
            None
        };
        let label = match quality {
            Some(q) => format!("{rate} Hz ({q})"),
            None => format!("{rate} Hz"),
        };
        fields.push((i18n("Sample Rate"), label));
    }
    if let Some(ch) = stream["channels"].as_i64() {
        let ch_desc = match ch {
            1 => format!("1 ({})", i18n("Mono")),
            2 => format!("2 ({})", i18n("Stereo")),
            6 => format!("6 ({})", i18n("5.1 Surround")),
            8 => format!("8 ({})", i18n("7.1 Surround")),
            n => n.to_string(),
        };
        fields.push((i18n("Channels"), ch_desc));
    }
    if let Some(br) = stream["bit_rate"].as_str()
        && let Ok(brf) = br.parse::<f64>()
    {
        fields.push((i18n("Bitrate"), format!("{:.2} kbps", brf / 1000.0)));
    }
    if let Some(lang) = stream["tags"]["language"].as_str() {
        fields.push((i18n("Language"), lang.to_uppercase()));
    }
    if let Some(title) = stream["tags"]["title"].as_str() {
        fields.push((i18n("Title"), title.to_string()));
    }
    fields
}

pub(super) fn parse_subtitle_stream(stream: &Value) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    if let Some(cn) = stream["codec_name"].as_str() {
        fields.push((i18n("Codec"), cn.to_uppercase()));
    }
    if let Some(lang) = stream["tags"]["language"].as_str() {
        fields.push((i18n("Language"), lang.to_uppercase()));
    }
    if let Some(title) = stream["tags"]["title"].as_str() {
        fields.push((i18n("Title"), title.to_string()));
    }
    fields
}

/// `HH:MM:SS.mmm`, the form ffmpeg itself prints durations in.
fn ffmpeg_timestamp(s: f64) -> String {
    let total_ms = (s.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        total_ms / 3_600_000,
        total_ms / 60_000 % 60,
        total_ms / 1000 % 60,
        total_ms % 1000
    )
}
