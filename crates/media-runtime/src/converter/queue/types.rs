//! Queue value types: item status, per-file video overrides, segment-output
//! mode, queue-item record, and the worker→UI message variants used by
//! `drain_worker_messages`.

use crate::converter::ffmpeg::TrimRange;
use crate::converter::ffmpeg::progress::FfmpegProgressSnapshot;
use crate::converter::profiles;
use crate::converter::subtitle::SubtitleInput;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct ConversionResumeContext {
    pub job_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionStatus {
    Pending,
    Converting,
    Done,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VideoOverrides {
    #[serde(default)]
    pub brightness: f64,
    #[serde(default = "neutral_multiplier")]
    pub contrast: f64,
    #[serde(default = "neutral_multiplier")]
    pub saturation: f64,
    #[serde(default)]
    pub hue: f64,
    #[serde(default)]
    pub rotation: i32,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
}

impl Default for VideoOverrides {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: neutral_multiplier(),
            saturation: neutral_multiplier(),
            hue: 0.0,
            rotation: 0,
            flip_h: false,
            flip_v: false,
        }
    }
}

fn neutral_multiplier() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SegmentOutputMode {
    #[default]
    Join,
    Split,
}

#[derive(Debug, Clone)]
pub struct ConversionQueueEntry {
    pub id: u64,
    pub input: PathBuf,
    pub output: PathBuf,
    pub status: ConversionStatus,
    pub duration_secs: f64,
    pub profile_name: String,
    pub segments: Vec<TrimRange>,
    pub segment_mode: SegmentOutputMode,
    pub subtitle: SubtitleInput,
    pub audio_track: Option<u32>,
    pub video_track: Option<u32>,
    pub crop: Option<profiles::CropRect>,
    pub video_overrides: Option<VideoOverrides>,
    // Drives NR mono split/merge.
    pub input_audio_channels: u32,
    // If background metadata has not arrived, the conversion worker probes
    // inline before building the command.
    pub probed: bool,
}

#[derive(Debug, Clone)]
pub enum QueueMessage {
    Progress {
        entry_id: u64,
        progress_snapshot: FfmpegProgressSnapshot,
    },
    // `command` carries the rendered ffmpeg invocation for the details panel.
    Started {
        entry_id: u64,
        command: String,
    },
    Done {
        entry_id: u64,
    },
    Failed {
        entry_id: u64,
        error: String,
    },
    AllDone,
    Cancelled {
        entry_id: u64,
    },
    GpuFallback {
        entry_id: u64,
        stage: &'static str,
    },
    StderrLog {
        entry_id: u64,
        lines: Vec<String>,
    },
    StderrLine {
        entry_id: u64,
        line: String,
    },
    /// Background duration and audio-channel count for a newly added item.
    Probed {
        /// Item id.
        entry_id: u64,
        /// Total duration in seconds (0.0 when unknown).
        duration_secs: f64,
        /// Channel count of the first audio stream (0 when unknown).
        input_audio_channels: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::VideoOverrides;

    #[test]
    fn video_overrides_default_keeps_color_axes_neutral() {
        let overrides = VideoOverrides::default();

        assert_eq!(overrides.brightness, 0.0);
        assert_eq!(overrides.contrast, 1.0);
        assert_eq!(overrides.saturation, 1.0);
        assert_eq!(overrides.hue, 0.0);
    }

    #[test]
    fn video_overrides_from_old_json_defaults_missing_color_axes_to_neutral() {
        let overrides: VideoOverrides = serde_json::from_str(
            r#"{
                "brightness": 0.2,
                "hue": -0.1,
                "rotation": 90,
                "flip_h": true,
                "flip_v": false
            }"#,
        )
        .unwrap();

        assert_eq!(overrides.brightness, 0.2);
        assert_eq!(overrides.contrast, 1.0);
        assert_eq!(overrides.saturation, 1.0);
        assert_eq!(overrides.hue, -0.1);
        assert_eq!(overrides.rotation, 90);
        assert!(overrides.flip_h);
        assert!(!overrides.flip_v);
    }
}
