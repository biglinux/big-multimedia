//! Shared fixtures for ffmpeg argument-builder tests — dummy profiles, CPU job.

use super::*;
use crate::cast::gpu::{GpuDevice, GpuType};
use crate::converter::profiles::Profile;

mod audio;
mod command;
mod encoder;
mod video;

fn dummy_profile() -> Profile {
    Profile {
        profile: profiles::ProfileMeta::default(),
        video: profiles::VideoConfig::default(),
        audio: profiles::AudioConfig::default(),
        subtitles: profiles::SubtitleConfig::default(),
        output: profiles::OutputConfig::default(),
    }
}

fn cpu_device() -> GpuDevice {
    GpuDevice {
        gpu_type: GpuType::Cpu,
        name: "CPU".into(),
        device_path: String::new(),
    }
}

fn base_job(profile: Profile) -> FfmpegJob {
    FfmpegJob {
        input: PathBuf::from("/tmp/test.mp4"),
        output: PathBuf::from("/tmp/out.mp4"),
        profile,
        gpu: cpu_device(),
        stage: GpuStage::Software,
        trim: TrimRange::default(),
        subtitle: SubtitleInput::None,
        audio_track: None,
        video_track: None,
        crop: None,
        subtitle_streams: Vec::new(),
        skip_video_filters: false,
        video_overrides: None,
        input_audio_channels: 2,
        audio_only: false,
    }
}

fn job_with_filters(filters: profiles::VideoFilters, stage: GpuStage) -> FfmpegJob {
    let mut profile = dummy_profile();
    profile.video.filters = filters;
    let mut job = base_job(profile);
    job.stage = stage;
    job
}

fn args(cmd: &Command) -> Vec<String> {
    cmd.get_args()
        .map(|arg| arg.to_string_lossy().into())
        .collect()
}
