//! Structural validation against an explicit export plan, not just file size.
//! This does not prove decoded/perceptual equality. Duration tolerance below is
//! a conservative project policy; long-GOP copy cuts can legitimately require
//! a precise re-encode instead of relaxing the check without telling the user.
use super::ConversionQueueEntry;
use crate::converter::{
    ffmpeg::{TrimRange, progress},
    profiles::Profile,
    subtitle::{SubtitleInput, SubtitleStream},
};
use crate::error::{Error, Result};
use crate::i18n::i18n;
use serde_json::Value;
use std::path::Path;

fn invalid(message: &str) -> Error {
    Error::ffmpeg("ffprobe", None, message.to_owned())
}
pub(super) fn number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}
pub(super) fn validate_ranges(ranges: &[TrimRange], duration: f64) -> Result<()> {
    if ranges.is_empty() || ranges.len() > 10_000 || !duration.is_finite() || duration < 0.0 {
        return Err(invalid(&i18n(
            "Invalid export duration or too many cut ranges",
        )));
    }
    for range in ranges {
        let start = range.start.unwrap_or(0.0);
        let end = range.end.unwrap_or(duration);
        if range.start.is_none() && range.end.is_none() {
            continue;
        }
        if duration <= 0.0
            || !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || end <= start
            || end > duration
        {
            return Err(invalid(&i18n(
                "Cuts must be inside the verified media duration",
            )));
        }
    }
    Ok(())
}
fn codec(request: &str, source: &str) -> String {
    match request {
        "copy" => source,
        "h265" => "hevc",
        "pcm" => "pcm_s16le",
        "libopus" => "opus",
        "libvorbis" => "vorbis",
        "libmp3lame" => "mp3",
        other => other,
    }
    .to_owned()
}
#[derive(Clone, Debug)]
pub(super) struct Expectation {
    video: Option<String>,
    audio: Option<String>,
    subtitles: usize,
    pub(super) duration: f64,
}
impl Expectation {
    pub(super) fn for_entry(
        entry: &ConversionQueueEntry,
        profile: &Profile,
        audio_only: bool,
        input: &Value,
        subtitles: &[SubtitleStream],
    ) -> Result<Self> {
        let streams = input
            .get("streams")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(&i18n("Source streams could not be verified")))?;
        if streams.len() > 4096 {
            return Err(invalid(&i18n("Too many source streams")));
        }
        let selected_video = streams
            .iter()
            .filter(|s| s["codec_type"] == "video")
            .nth(entry.video_track.unwrap_or(0) as usize);
        let video = if audio_only {
            None
        } else {
            let stream = selected_video
                .ok_or_else(|| invalid(&i18n("The selected video stream is missing")))?;
            if stream
                .pointer("/disposition/attached_pic")
                .and_then(Value::as_i64)
                == Some(1)
            {
                return Err(invalid(&i18n(
                    "A cover image is not the requested video stream",
                )));
            }
            let source = stream["codec_name"]
                .as_str()
                .ok_or_else(|| invalid(&i18n("Unknown source video codec")))?;
            Some(codec(&profile.video.codec, source))
        };
        let selected_audio = streams
            .iter()
            .filter(|s| s["codec_type"] == "audio")
            .nth(entry.audio_track.unwrap_or(0) as usize);
        let audio = if profile.audio.handling == "none" {
            None
        } else if let Some(stream) = selected_audio {
            let source = stream["codec_name"]
                .as_str()
                .ok_or_else(|| invalid(&i18n("Unknown source audio codec")))?;
            Some(codec(
                if profile.audio.handling == "copy" {
                    "copy"
                } else {
                    &profile.audio.codec
                },
                source,
            ))
        } else if audio_only || entry.audio_track.is_some() {
            return Err(invalid(&i18n("The selected audio stream is missing")));
        } else {
            None
        };
        if video.is_none() && audio.is_none() {
            return Err(invalid(&i18n("The plan has no audio or video output")));
        }
        let wanted_subtitles = if audio_only || profile.subtitles.handling != "embedded" {
            0
        } else if matches!(entry.subtitle, SubtitleInput::ExternalFile(_)) {
            1
        } else {
            let original_count = streams
                .iter()
                .filter(|s| s["codec_type"] == "subtitle")
                .count();
            if subtitles.len() != original_count {
                return Err(invalid(&i18n("Subtitle streams could not be verified")));
            }
            if matches!(
                crate::converter::profiles::resolve_container(profile),
                "mp4" | "mov" | "m4v"
            ) {
                subtitles.iter().filter(|s| !s.is_bitmap).count()
            } else {
                subtitles.len()
            }
        };
        Ok(Self {
            video,
            audio,
            subtitles: wanted_subtitles,
            duration: entry.duration_secs,
        })
    }
}
pub(super) fn validate_probe(output: &Value, wanted: &Expectation) -> Result<()> {
    let streams = output["streams"]
        .as_array()
        .ok_or_else(|| invalid(&i18n("Export has no readable stream list")))?;
    if streams.len() > 4096 {
        return Err(invalid(&i18n("Too many output streams")));
    }
    for (kind, codec) in [("video", &wanted.video), ("audio", &wanted.audio)] {
        let selected: Vec<_> = streams
            .iter()
            .filter(|s| {
                s["codec_type"] == kind
                    && !(kind == "video"
                        && s.pointer("/disposition/attached_pic")
                            .and_then(Value::as_i64)
                            == Some(1))
            })
            .collect();
        if selected.len() != usize::from(codec.is_some()) {
            return Err(invalid(&i18n(
                "Exported stream count differs from the plan",
            )));
        }
        if let (Some(codec), Some(stream)) = (codec, selected.first()) {
            if stream["codec_name"].as_str() != Some(codec.as_str()) {
                return Err(invalid(&i18n("Exported codec differs from the plan")));
            }
            if kind == "video"
                && (stream["width"].as_u64().unwrap_or(0) == 0
                    || stream["height"].as_u64().unwrap_or(0) == 0)
            {
                return Err(invalid(&i18n("Exported video dimensions are invalid")));
            }
            if kind == "audio" && stream["channels"].as_u64().unwrap_or(0) == 0 {
                return Err(invalid(&i18n("Exported audio has no channels")));
            }
        }
    }
    if streams
        .iter()
        .filter(|s| s["codec_type"] == "subtitle")
        .count()
        != wanted.subtitles
    {
        return Err(invalid(&i18n(
            "Exported subtitle count differs from the plan",
        )));
    }
    let duration = number(output.pointer("/format/duration"))
        .filter(|n| *n > 0.0)
        .ok_or_else(|| invalid(&i18n("Exported duration could not be verified")))?;
    if wanted.duration > 0.0 {
        let tolerance = (wanted.duration * 0.02).clamp(0.05, 2.0);
        if (duration - wanted.duration).abs() > tolerance {
            return Err(invalid(&i18n(
                "Exported duration differs from the plan. Use precise re-encoding for exact cuts",
            )));
        }
    }
    Ok(())
}
pub(super) fn validate_output_cancellable(
    path: &Path,
    wanted: &Expectation,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(invalid(&i18n("Exported file is empty or is not regular")));
    }
    let probe = progress::probe_media_metadata_cancellable(path, cancel)?;
    validate_probe(&probe, wanted)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_ranges_do_not_silently_clamp() {
        for range in [
            TrimRange {
                start: Some(f64::NAN),
                end: Some(2.0),
            },
            TrimRange {
                start: Some(4.0),
                end: Some(12.0),
            },
            TrimRange {
                start: Some(3.0),
                end: Some(3.0),
            },
        ] {
            assert!(validate_ranges(&[range], 10.0).is_err());
        }
        assert!(validate_ranges(&[TrimRange::default()], 0.0).is_ok());
    }
    #[test]
    fn audio_is_not_success_for_a_video_plan() {
        let plan = Expectation {
            video: Some("h264".into()),
            audio: None,
            subtitles: 0,
            duration: 10.0,
        };
        let audio = serde_json::json!({"streams":[{"codec_type":"audio","codec_name":"aac","channels":2}],"format":{"duration":"10.0"}});
        assert!(validate_probe(&audio, &plan).is_err());
        let cover = serde_json::json!({"streams":[{"codec_type":"video","codec_name":"h264","width":32,"height":32,"disposition":{"attached_pic":1}}],"format":{"duration":"10"}});
        assert!(validate_probe(&cover, &plan).is_err());
    }
    #[test]
    fn expected_codec_streams_and_duration_are_required() {
        let plan = Expectation {
            video: None,
            audio: Some("aac".into()),
            subtitles: 0,
            duration: 10.0,
        };
        let mut probe = serde_json::json!({"streams":[{"codec_type":"audio","codec_name":"aac","channels":2}],"format":{"duration":"10.0"}});
        assert!(validate_probe(&probe, &plan).is_ok());
        probe["format"]["duration"] = serde_json::json!("1.0");
        assert!(validate_probe(&probe, &plan).is_err());
        let short = Expectation {
            duration: 1.0,
            ..plan
        };
        probe["format"]["duration"] = serde_json::json!("0.1");
        assert!(validate_probe(&probe, &short).is_err());
    }
}
