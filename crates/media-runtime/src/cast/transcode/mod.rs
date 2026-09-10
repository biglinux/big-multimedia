//! FFmpeg transcoding for Chromecast. Builds audio (EQ, voice, volume,
//! tempo) and video (colour, flip, rotate, subtitles) filter chains and
//! picks CPU or GPU (VAAPI/NVENC) codecs. Runs FFmpeg as a child process
//! emitting fragmented MP4 to stdout for pipe-based HTTP streaming.

mod args;
mod probe;
mod session;
#[cfg(test)]
mod tests;
mod types;

use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tempfile::TempDir;

use crate::cast::gpu::GpuType;

pub use types::TranscodeFilters;

const CAST_HIGHPASS_HZ: u32 = 80;

/// Transcoding stage for the 3-stage GPU fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscodeStage {
    /// Stage 1: Full GPU decode + encode (hwaccel_output_format set, fastest).
    FullGpu,
    /// Stage 2: Software decode + GPU encode (partial GPU, more compatible).
    PartialGpu,
    /// Stage 3: Full software (libx264, universal fallback).
    Software,
}

/// Manages a running FFmpeg transcode process.
pub struct TranscodeSession {
    /// Path to the input file.
    input_path: String,
    /// Selected GPU for encoding.
    gpu_type: GpuType,
    /// GPU device path (e.g. "/dev/dri/renderD128").
    device_path: String,
    /// Active filter configuration.
    filters: TranscodeFilters,
    /// Running FFmpeg child process.
    child: Option<Child>,
    /// Stop flag shared with the HTTP server.
    _stop: Arc<AtomicBool>,
    /// Path to extracted text subtitle temp file (avoids re-parsing entire container).
    /// `None` if not extracted. Once set, reused for all subsequent starts with the same subtitle.
    extracted_sub_path: Option<String>,
    /// The subtitle track index for which the extraction was done.
    extracted_sub_track: Option<usize>,
    /// Video dimensions (from probe), used to scale PGS subtitle overlays.
    video_width: u32,
    video_height: u32,
    /// Owned 0700 temp dir for cast scratch files (cmd log, stderr log,
    /// extracted subtitle). Prevents `/tmp` symlink races: attacker cannot
    /// pre-create files inside a dir they don't own. Auto-removed on drop.
    temp_dir: Option<TempDir>,
}
