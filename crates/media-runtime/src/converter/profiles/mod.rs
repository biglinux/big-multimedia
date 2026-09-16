//! TOML-based encoding profile system.
//!
//! Profiles are loaded from two directories:
//! - System: `/usr/share/big-media-runtime/profiles/` (read-only)
//! - User:   `~/.config/big-media-runtime/profiles/` (read-write)
//!
//! User profiles override system profiles with the same filename.

mod io;
mod output;
mod quality;
#[cfg(test)]
mod tests;
mod types;

pub use io::{load_all_profiles, load_profile, save_user_profile, save_user_profile_async};
pub use output::{resolve_audio_output_name, resolve_container, resolve_output_name};
pub use quality::{preset_to_ffmpeg, quality_to_crf};
pub use types::{
    AudioConfig, AudioEffectsConfig, CropRect, GpuConfig, NoiseReductionConfig, OutputConfig,
    Profile, ProfileMeta, SubtitleConfig, VideoConfig, VideoFilters,
};
