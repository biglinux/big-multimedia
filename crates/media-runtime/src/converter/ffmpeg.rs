//! FFmpeg command builder. Typed args via `Command` (no shell, immune to
//! quoting bugs). Three-stage GPU fallback: FullGpu (HW decode + HW
//! encode) → PartialGpu (SW decode + HW encode) → Software (CPU only).
//! Per-encoder quality maps live here because each vendor uses a different
//! scale (NVIDIA CQ, VAAPI global_quality, QSV global_quality, ProRes
//! profile number).

use crate::cast::gpu::{GpuDevice, GpuType};
use crate::converter::profiles::{self, CropRect, Profile};
use crate::converter::queue::VideoOverrides;
use crate::converter::subtitle::{SubtitleInput, SubtitleStream};
use big_audio_effects::equalizer::{CONVERTER_EQ_BANDWIDTH_OCTAVES, EQ_BAND_FREQUENCIES_HZ};
use log::info;
use std::path::PathBuf;
use std::process::{Command, Stdio};

mod audio;
mod command;
pub mod progress;
mod quality;
#[cfg(test)]
mod tests;
mod video;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuStage {
    FullGpu,
    PartialGpu,
    Software,
}

// None on either side = open-ended (beginning / EOF).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TrimRange {
    pub start: Option<f64>,
    pub end: Option<f64>,
}

impl TrimRange {
    /// Duration actually processed for this range against the full media
    /// `total_duration`. This — NOT the whole-media duration — is what drives
    /// the progress percent: `-ss <start>` is placed before `-i` (input seek),
    /// so ffmpeg's `-progress` `out_time` runs `0..=effective_duration`. An
    /// open-ended side falls back to the media bounds; returns `0.0` when the
    /// media duration is unknown and the range cannot otherwise be measured
    /// (the parser then leaves percent/ETA unset rather than dividing by zero).
    #[must_use]
    pub fn effective_duration(&self, total_duration: f64) -> f64 {
        let start = self.start.unwrap_or(0.0).max(0.0);
        let end = match self.end {
            Some(end) if total_duration > 0.0 => end.min(total_duration),
            Some(end) => end,
            None => total_duration,
        };
        (end - start).max(0.0)
    }
}

#[derive(Debug, Clone)]
pub struct FfmpegJob {
    pub input: PathBuf,
    pub output: PathBuf,
    pub profile: Profile,
    pub gpu: GpuDevice,
    pub stage: GpuStage,
    pub trim: TrimRange,
    pub subtitle: SubtitleInput,
    pub audio_track: Option<u32>,
    pub video_track: Option<u32>,
    pub crop: Option<CropRect>,
    pub video_overrides: Option<VideoOverrides>,
    // Drives NR mono split/merge.
    pub input_audio_channels: u32,
    // Lets the command builder filter out bitmap subs for MP4/MOV.
    pub subtitle_streams: Vec<SubtitleStream>,
    pub audio_only: bool,
}

fn nvidia_cq(codec: &str, quality: &str) -> u32 {
    match codec {
        "h265" => match quality {
            "veryhigh" => 22,
            "high" => 25,
            "low" => 31,
            "verylow" => 34,
            "superlow" => 37,
            _ => 28,
        },
        _ => match quality {
            "veryhigh" => 19,
            "high" => 24,
            "low" => 31,
            "verylow" => 34,
            "superlow" => 38,
            _ => 28,
        },
    }
}

fn nvidia_preset_num(preset: &str) -> u32 {
    match preset {
        "ultrafast" | "veryfast" => 1,
        "fast" => 2,
        "slightlyfast" | "faster" => 3,
        "slow" => 5,
        "veryslow" => 6,
        _ => 4,
    }
}

fn vaapi_global_quality(codec: &str, quality: &str) -> u32 {
    match codec {
        "h265" => match quality {
            "veryhigh" => 22,
            "high" => 25,
            "low" => 31,
            "verylow" => 34,
            "superlow" => 37,
            _ => 28,
        },
        _ => match quality {
            "veryhigh" => 18,
            "high" => 21,
            "low" => 27,
            "verylow" => 30,
            "superlow" => 33,
            _ => 24,
        },
    }
}

fn intel_global_quality(_codec: &str, quality: &str) -> u32 {
    match quality {
        "veryhigh" => 18,
        "high" => 21,
        "low" => 27,
        "verylow" => 30,
        "superlow" => 33,
        _ => 24,
    }
}

fn prores_profile_num(quality: &str) -> u32 {
    match quality {
        "superlow" | "verylow" => 0,
        "low" => 1,
        "high" => 3,
        "veryhigh" => 4,
        _ => 2,
    }
}

/// `HH:MM:SS.mmm`: the form ffmpeg's `-ss`/`-t` take, also shown in the media
/// details dialog. Rounded once to whole milliseconds, so 59.9996 s becomes
/// `00:01:00.000` rather than the out-of-range `00:00:60.000`.
pub(crate) fn ffmpeg_timestamp(s: f64) -> String {
    let total_ms = (s.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        total_ms / 3_600_000,
        total_ms / 60_000 % 60,
        total_ms / 1000 % 60,
        total_ms % 1000
    )
}
