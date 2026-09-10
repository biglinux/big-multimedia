//! Converter module: queue-driven batch video/audio conversion via ffmpeg
//! with NVENC/VAAPI/QSV → software fallback, profile-based output config,
//! shared queue, profile, validation, and ffmpeg command code.

pub mod ffmpeg;

pub mod file_validator;
pub mod profiles;
pub mod queue;
pub mod runtime_config;
pub mod subtitle;
