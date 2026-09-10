//! Profile schema (serde TOML): Profile owns video/audio/subtitles/output
//! sub-configs, each with `#[serde(default)]` so partial TOML keeps working.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default, alias = "meta")]
    pub profile: ProfileMeta,
    #[serde(default)]
    pub video: VideoConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default, alias = "subtitle")]
    pub subtitles: SubtitleConfig,
    #[serde(default)]
    pub output: OutputConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMeta {
    #[serde(default = "default_profile_name")]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default = "default_category")]
    pub category: String,
}

fn default_profile_name() -> String {
    "Custom".into()
}

fn default_category() -> String {
    "general".into()
}

impl Default for ProfileMeta {
    fn default() -> Self {
        Self {
            name: default_profile_name(),
            description: String::new(),
            icon: String::new(),
            category: default_category(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    #[serde(default = "default_codec")]
    pub codec: String,
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default = "default_preset")]
    pub preset: String,
    #[serde(default = "default_auto")]
    pub pixel_format: String,
    #[serde(default = "default_auto")]
    pub container: String,
    #[serde(default)]
    pub gpu: GpuConfig,
    #[serde(default)]
    pub filters: VideoFilters,
}

fn default_codec() -> String {
    "h264".into()
}

fn default_quality() -> String {
    "good".into()
}

fn default_preset() -> String {
    "balanced".into()
}

fn default_auto() -> String {
    "auto".into()
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            codec: default_codec(),
            quality: default_quality(),
            preset: default_preset(),
            pixel_format: default_auto(),
            container: default_auto(),
            gpu: GpuConfig::default(),
            filters: VideoFilters::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    #[serde(default = "default_auto")]
    pub mode: String,
    #[serde(default)]
    pub partial: bool,
    #[serde(default = "default_auto")]
    pub device: String,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            mode: default_auto(),
            partial: false,
            device: default_auto(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFilters {
    #[serde(default)]
    pub brightness: f64,
    #[serde(default = "one_f64")]
    pub contrast: f64,
    #[serde(default = "one_f64")]
    pub saturation: f64,
    #[serde(default)]
    pub hue: f64,
    #[serde(default = "default_auto")]
    pub resolution: String,
    #[serde(default)]
    pub rotation: i32,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    #[serde(default)]
    pub crop: Option<CropRect>,
}

impl Default for VideoFilters {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            hue: 0.0,
            resolution: default_auto(),
            rotation: 0,
            flip_h: false,
            flip_v: false,
            crop: None,
        }
    }
}

fn one_f64() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropRect {
    pub w: u32,
    pub h: u32,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    #[serde(default = "default_copy")]
    pub handling: String,
    #[serde(default = "default_aac")]
    pub codec: String,
    #[serde(default = "default_bitrate")]
    pub bitrate: String,
    #[serde(default = "default_auto")]
    pub channels: String,
    #[serde(default)]
    pub noise_reduction: NoiseReductionConfig,
    #[serde(default)]
    pub effects: AudioEffectsConfig,
}

fn default_copy() -> String {
    "copy".into()
}

fn default_aac() -> String {
    "aac".into()
}

fn default_bitrate() -> String {
    "128k".into()
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            handling: default_copy(),
            codec: default_aac(),
            bitrate: default_bitrate(),
            channels: default_auto(),
            noise_reduction: NoiseReductionConfig::default(),
            effects: AudioEffectsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoiseReductionConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "one_f64")]
    pub strength: f64,
    #[serde(default)]
    pub model: i32,
    #[serde(default = "half_f64")]
    pub speech_strength: f64,
    #[serde(default = "half_f64")]
    pub voice_recovery: f64,
}

fn half_f64() -> f64 {
    0.5
}

impl Default for NoiseReductionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 1.0,
            model: 0,
            speech_strength: 0.5,
            voice_recovery: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioEffectsConfig {
    #[serde(default)]
    pub gate_enabled: bool,
    #[serde(default = "half_f64")]
    pub gate_intensity: f64,
    #[serde(default)]
    pub compressor_enabled: bool,
    #[serde(default = "half_f64")]
    pub compressor_intensity: f64,
    #[serde(default)]
    pub hpf_enabled: bool,
    #[serde(default = "default_hpf_freq")]
    pub hpf_frequency: u32,
    #[serde(default)]
    pub normalize: bool,
    #[serde(default)]
    pub eq_enabled: bool,
    #[serde(default = "default_eq_preset")]
    pub eq_preset: String,
    #[serde(default)]
    pub eq_bands: Vec<f64>,
    // Audio converter only.
    #[serde(default = "one_f64")]
    pub volume: f64,
    #[serde(default = "one_f64")]
    pub speed: f64,
}

fn default_hpf_freq() -> u32 {
    80
}

fn default_eq_preset() -> String {
    "flat".into()
}

impl Default for AudioEffectsConfig {
    fn default() -> Self {
        Self {
            gate_enabled: false,
            gate_intensity: 0.5,
            compressor_enabled: false,
            compressor_intensity: 0.5,
            hpf_enabled: false,
            hpf_frequency: 80,
            normalize: false,
            eq_enabled: false,
            eq_preset: default_eq_preset(),
            eq_bands: Vec::new(),
            volume: 1.0,
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleConfig {
    #[serde(default = "default_embedded", alias = "mode")]
    pub handling: String,
    #[serde(default)]
    pub extract_only: bool,
    #[serde(default = "default_srt")]
    pub format: String,
}

fn default_embedded() -> String {
    "embedded".into()
}

fn default_srt() -> String {
    "srt".into()
}

impl Default for SubtitleConfig {
    fn default() -> Self {
        Self {
            handling: default_embedded(),
            extract_only: false,
            format: default_srt(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(default = "default_auto")]
    pub container: String,
    #[serde(default = "default_naming")]
    pub naming: String,
}

fn default_naming() -> String {
    "{name}.{ext}".into()
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            container: default_auto(),
            naming: default_naming(),
        }
    }
}
