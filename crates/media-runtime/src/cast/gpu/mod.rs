//! GPU detection and ffmpeg encoder probing for cast/transcode pipelines.

mod detect;
mod ffmpeg;
mod types;

pub use detect::{detect_all_gpus, detect_gpu_device};
pub use ffmpeg::{ffmpeg_available, ffmpeg_has_encoder};
pub use types::{GpuDevice, GpuType};
