//! Runtime config: file-format extension sets + color-adjustment calibration.
//! Layered TOML: system install, dev fallback beside the binary, then user
//! config. Each step merges into compiled-in defaults so a missing or malformed
//! file never prevents startup.

use log::{info, warn};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const SYSTEM_CONFIG_PATH: &str = crate::config::runtime::CONVERTER_CONFIG_PATH;
const USER_CONFIG_RELATIVE: &str = crate::config::runtime::CONVERTER_CONFIG_RELATIVE;

const DEFAULT_VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "webm", "avi", "mov", "wmv", "flv", "ts", "m2ts", "mts", "m4v", "mpg", "mpeg",
    "3gp", "ogv", "vob", "f4v", "divx", "asf",
];
const DEFAULT_AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "aac", "ogg", "opus", "wma", "m4a", "aiff", "ape", "wv", "ac3", "dts",
    "mka", "aif", "oga",
];
const DEFAULT_FLOAT_THRESHOLD: f64 = 0.001;
const DEFAULT_SATURATION_NEUTRAL: f64 = 1.0;
const DEFAULT_SATURATION_ABOVE_SCALE: f64 = 1.5;
const DEFAULT_CONTRAST_NEUTRAL: f64 = 1.0;
const DEFAULT_CONTRAST_BELOW_SCALE: f64 = 1.0;
const DEFAULT_CONTRAST_ABOVE_SCALE: f64 = 1.0;

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    file_formats: RawFileFormats,
    #[serde(default)]
    adjustments: RawAdjustments,
}

#[derive(Debug, Default, Deserialize)]
struct RawFileFormats {
    #[serde(default)]
    video: Option<Vec<String>>,
    #[serde(default)]
    audio: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAdjustments {
    #[serde(default)]
    float_threshold: Option<f64>,
    #[serde(default)]
    saturation: Option<RawSaturation>,
    #[serde(default)]
    contrast: Option<RawContrast>,
}

#[derive(Debug, Deserialize)]
struct RawSaturation {
    #[serde(default)]
    neutral: Option<f64>,
    #[serde(default)]
    above_neutral_scale: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawContrast {
    #[serde(default)]
    neutral: Option<f64>,
    #[serde(default)]
    below_neutral_scale: Option<f64>,
    #[serde(default)]
    above_neutral_scale: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SaturationMapping {
    pub neutral: f64,
    pub above_neutral_scale: f64,
}

impl SaturationMapping {
    // Below neutral: 1:1. Above neutral: delta scaled by above_neutral_scale.
    #[must_use]
    pub fn map_preview_to_ffmpeg(&self, preview: f64) -> f64 {
        if preview >= self.neutral {
            self.neutral + (preview - self.neutral) * self.above_neutral_scale
        } else {
            preview
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContrastMapping {
    pub neutral: f64,
    pub below_neutral_scale: f64,
    pub above_neutral_scale: f64,
}

impl ContrastMapping {
    // mpv exposes contrast as -100..=100; converter UIs normalize that to
    // 0.0..=2.0. FFmpeg's eq filter expects a contrast multiplier, so this
    // keeps preview-space values in one place and allows runtime calibration.
    #[must_use]
    pub fn map_preview_to_ffmpeg(&self, preview: f64) -> f64 {
        if preview >= self.neutral {
            self.neutral + (preview - self.neutral) * self.above_neutral_scale
        } else {
            self.neutral - (self.neutral - preview) * self.below_neutral_scale
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub video_extensions: HashSet<String>,
    pub audio_extensions: HashSet<String>,
    pub float_threshold: f64,
    pub saturation: SaturationMapping,
    pub contrast: ContrastMapping,
}

impl RuntimeConfig {
    fn defaults() -> Self {
        Self {
            video_extensions: DEFAULT_VIDEO_EXTS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            audio_extensions: DEFAULT_AUDIO_EXTS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            float_threshold: DEFAULT_FLOAT_THRESHOLD,
            saturation: SaturationMapping {
                neutral: DEFAULT_SATURATION_NEUTRAL,
                above_neutral_scale: DEFAULT_SATURATION_ABOVE_SCALE,
            },
            contrast: ContrastMapping {
                neutral: DEFAULT_CONTRAST_NEUTRAL,
                below_neutral_scale: DEFAULT_CONTRAST_BELOW_SCALE,
                above_neutral_scale: DEFAULT_CONTRAST_ABOVE_SCALE,
            },
        }
    }

    fn apply(&mut self, raw: RawConfig) {
        if let Some(v) = raw.file_formats.video {
            self.video_extensions = v.into_iter().map(|s| s.to_lowercase()).collect();
        }
        if let Some(a) = raw.file_formats.audio {
            self.audio_extensions = a.into_iter().map(|s| s.to_lowercase()).collect();
        }
        if let Some(t) = raw.adjustments.float_threshold {
            self.float_threshold = t;
        }
        if let Some(sat) = raw.adjustments.saturation {
            if let Some(n) = sat.neutral {
                self.saturation.neutral = n;
            }
            if let Some(s) = sat.above_neutral_scale {
                self.saturation.above_neutral_scale = s;
            }
        }
        if let Some(contrast) = raw.adjustments.contrast {
            if let Some(n) = contrast.neutral {
                self.contrast.neutral = n;
            }
            if let Some(s) = contrast.below_neutral_scale {
                self.contrast.below_neutral_scale = s;
            }
            if let Some(s) = contrast.above_neutral_scale {
                self.contrast.above_neutral_scale = s;
            }
        }
    }

    #[must_use]
    pub fn has_video_ext(&self, ext: &str) -> bool {
        self.video_extensions.contains(&ext.to_lowercase())
    }

    #[must_use]
    pub fn has_audio_ext(&self, ext: &str) -> bool {
        self.audio_extensions.contains(&ext.to_lowercase())
    }
}

fn user_config_path() -> Option<PathBuf> {
    big_os_kit::xdg_dirs::config_dir().map(|d| d.join(USER_CONFIG_RELATIVE))
}

fn load_from(path: &Path, target: &mut RuntimeConfig) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    match toml::from_str::<RawConfig>(&content) {
        Ok(raw) => {
            target.apply(raw);
            info!("Loaded converter runtime config: {}", path.display());
            true
        }
        Err(e) => {
            warn!("Skip runtime config {} ({e})", path.display());
            false
        }
    }
}

fn build() -> RuntimeConfig {
    let mut config = RuntimeConfig::defaults();
    let mut loaded = load_from(Path::new(SYSTEM_CONFIG_PATH), &mut config);

    if !loaded && let Some(dev) = dev_config_path() {
        loaded = load_from(&dev, &mut config);
    }

    if let Some(user) = user_config_path()
        && user.is_file()
    {
        load_from(&user, &mut config);
    }

    if !loaded {
        info!("Using compiled-in converter runtime defaults");
    }
    config
}

fn dev_config_path() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        for rel in [
            "../../data/converter-runtime.toml",
            "../data/converter-runtime.toml",
            "data/converter-runtime.toml",
        ] {
            let p = exe_dir.join(rel);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let cwd = Path::new("data/converter-runtime.toml");
    if cwd.is_file() {
        return Some(cwd.to_path_buf());
    }
    None
}

static CONFIG: LazyLock<RuntimeConfig> = LazyLock::new(build);

#[must_use]
pub fn config() -> &'static RuntimeConfig {
    &CONFIG
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_legacy_hardcoded_video_set() {
        let runtime_config = RuntimeConfig::defaults();
        assert_eq!(runtime_config.video_extensions.len(), 19);
        for ext in [
            "mp4", "mkv", "webm", "avi", "mov", "wmv", "flv", "ts", "m2ts", "mts", "m4v", "mpg",
            "mpeg", "3gp", "ogv", "vob", "f4v", "divx", "asf",
        ] {
            assert!(runtime_config.has_video_ext(ext), "missing {ext}");
        }
    }

    #[test]
    fn defaults_match_legacy_hardcoded_audio_set() {
        let runtime_config = RuntimeConfig::defaults();
        assert_eq!(runtime_config.audio_extensions.len(), 16);
        for ext in [
            "mp3", "flac", "wav", "aac", "ogg", "opus", "wma", "m4a", "aiff", "ape", "wv", "ac3",
            "dts", "mka", "aif", "oga",
        ] {
            assert!(runtime_config.has_audio_ext(ext), "missing {ext}");
        }
    }

    #[test]
    fn extension_lookup_is_case_insensitive() {
        let runtime_config = RuntimeConfig::defaults();
        assert!(runtime_config.has_video_ext("MP4"));
        assert!(runtime_config.has_audio_ext("Mp3"));
    }

    #[test]
    fn saturation_below_neutral_is_identity() {
        let map = SaturationMapping {
            neutral: 1.0,
            above_neutral_scale: 1.5,
        };
        assert!((map.map_preview_to_ffmpeg(0.5) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn saturation_above_neutral_uses_scale() {
        let map = SaturationMapping {
            neutral: 1.0,
            above_neutral_scale: 1.5,
        };
        assert!((map.map_preview_to_ffmpeg(2.0) - 2.5).abs() < 1e-9);
        assert!((map.map_preview_to_ffmpeg(1.5) - 1.75).abs() < 1e-9);
    }

    #[test]
    fn contrast_mapping_scales_around_neutral() {
        let map = ContrastMapping {
            neutral: 1.0,
            below_neutral_scale: 0.5,
            above_neutral_scale: 1.5,
        };
        assert!((map.map_preview_to_ffmpeg(0.5) - 0.75).abs() < 1e-9);
        assert!((map.map_preview_to_ffmpeg(1.5) - 1.75).abs() < 1e-9);
    }

    #[test]
    fn raw_config_partial_override_keeps_defaults() {
        let mut runtime_config = RuntimeConfig::defaults();
        let raw: RawConfig = toml::from_str(
            r"
            [adjustments.saturation]
            above_neutral_scale = 2.0

            [adjustments.contrast]
            below_neutral_scale = 0.75
            ",
        )
        .unwrap();
        runtime_config.apply(raw);
        assert!((runtime_config.saturation.above_neutral_scale - 2.0).abs() < 1e-9);
        assert!((runtime_config.saturation.neutral - 1.0).abs() < 1e-9);
        assert!((runtime_config.contrast.below_neutral_scale - 0.75).abs() < 1e-9);
        assert!((runtime_config.contrast.neutral - 1.0).abs() < 1e-9);
        assert_eq!(runtime_config.video_extensions.len(), 19);
    }

    #[test]
    fn raw_config_replaces_extension_lists_when_present() {
        let mut runtime_config = RuntimeConfig::defaults();
        let raw: RawConfig = toml::from_str(
            r#"
            [file_formats]
            video = ["mp4", "MKV"]
            "#,
        )
        .unwrap();
        runtime_config.apply(raw);
        assert_eq!(runtime_config.video_extensions.len(), 2);
        assert!(runtime_config.has_video_ext("mkv"));
        assert!(runtime_config.has_video_ext("mp4"));
        assert!(!runtime_config.has_video_ext("avi"));
        assert_eq!(runtime_config.audio_extensions.len(), 16);
    }
}
