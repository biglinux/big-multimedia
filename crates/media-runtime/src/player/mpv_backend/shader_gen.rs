//! Generates customized GLSL shaders by substituting `#define` values into
//! templates from `SHADERS_DIR`, writing the result under `~/.config/`.
//! mpv loads from the user path rather than the system template.

use crate::config::SHADERS_DIR;
use log::{error, info};
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

fn user_shaders_dir() -> Option<PathBuf> {
    let dir = glib::user_config_dir().join(crate::config::video::USER_SHADERS_RELATIVE);
    fs::create_dir_all(&dir)
        .map_err(|e| error!("Cannot create shader dir {}: {e}", dir.display()))
        .ok()?;
    Some(dir)
}

/// Preserves trailing `//` comments on the line.
fn replace_define(source: &str, name: &str, new_value: &str) -> String {
    let prefix = format!("#define {name} ");
    let mut result = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&prefix) {
            let after_prefix = &trimmed[prefix.len()..];
            if let Some(comment_pos) = after_prefix.find("//") {
                let comment = &after_prefix[comment_pos..];
                let _ = writeln!(result, "{prefix}{new_value} {comment}");
            } else {
                let _ = writeln!(result, "{prefix}{new_value}");
            }
        } else {
            let _ = writeln!(result, "{line}");
        }
    }
    result
}

pub struct ShaderParam<'a> {
    pub define_name: &'a str,
    pub value: f64,
    /// Decimal places.
    pub precision: u8,
}

pub fn generate(filename: &str, params: &[ShaderParam<'_>]) -> Option<String> {
    let template_path = Path::new(SHADERS_DIR).join(filename);
    let source = fs::read_to_string(&template_path)
        .map_err(|e| {
            error!(
                "Cannot read shader template {}: {e}",
                template_path.display()
            );
        })
        .ok()?;

    let mut result = source;
    for p in params {
        let formatted = format!("{:.prec$}", p.value, prec = p.precision as usize);
        result = replace_define(&result, p.define_name, &formatted);
    }

    let out_dir = user_shaders_dir()?;
    let out_path = out_dir.join(filename);
    fs::write(&out_path, &result)
        .map_err(|e| error!("Cannot write shader {}: {e}", out_path.display()))
        .ok()?;

    let path_str = out_path.to_str()?.to_string();
    info!("Generated shader: {path_str}");
    Some(path_str)
}

/// Always generates: the file is tiny and writes are cheap. Falls back to the
/// system template path on error.
pub fn shader_path(filename: &str, params: &[ShaderParam<'_>]) -> String {
    if let Some(path) = generate(filename, params) {
        path
    } else {
        let sys = Path::new(SHADERS_DIR).join(filename);
        sys.to_string_lossy().into_owned()
    }
}

/// Anime4K v4.0 optimized presets:
/// `a` best for 1080p → 4K; `b` for 720p / low-quality (soft denoise + upscale);
/// `c` lightweight (denoise built into upscaler).
pub fn anime4k_preset_shaders(preset: &str) -> &[&str] {
    match preset {
        "a" => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Restore_CNN_VL.glsl",
            "Anime4K_Upscale_CNN_x2_VL.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
            "Anime4K_AutoDownscalePre_x4.glsl",
            "Anime4K_Upscale_CNN_x2_M.glsl",
        ],
        "b" => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Restore_CNN_Soft_VL.glsl",
            "Anime4K_Upscale_CNN_x2_VL.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
            "Anime4K_AutoDownscalePre_x4.glsl",
            "Anime4K_Upscale_CNN_x2_S.glsl",
        ],
        "c" => &[
            "Anime4K_Clamp_Highlights.glsl",
            "Anime4K_Upscale_Denoise_CNN_x2_M.glsl",
            "Anime4K_AutoDownscalePre_x2.glsl",
            "Anime4K_AutoDownscalePre_x4.glsl",
            "Anime4K_Upscale_CNN_x2_S.glsl",
        ],
        _ => &[],
    }
}

pub fn anime4k_dir() -> PathBuf {
    Path::new(SHADERS_DIR).join("anime4k")
}

// RAVU-Lite (bjin/mpv-prescalers, LGPL-3.0) is the only supported upscaler:
// fragment shaders compatible with vo=libmpv. ArtCNN (Artoriuz/ArtCNN) is
// excluded: its //!COMPUTE shaders need vo=gpu-next; vo=gpu asserts in copy_image.

pub fn ravu_dir() -> PathBuf {
    Path::new(SHADERS_DIR).join("ravu")
}

/// Only RAVU-Lite r4 ships: r2/r3 gain too little over lanczos.
pub fn neural_upscaler_filename(variant: &str) -> Option<&'static str> {
    match variant {
        "ravu-r4" => Some("ravu-lite-ar-r4.hook"),
        _ => None,
    }
}

pub fn neural_upscaler_path(variant: &str) -> Option<String> {
    if variant.is_empty() {
        return None;
    }
    let filename = neural_upscaler_filename(variant)?;
    let path = ravu_dir().join(filename);
    if path.exists() {
        Some(path.to_string_lossy().into_owned())
    } else {
        error!("Neural upscaler shader missing: {}", path.display());
        None
    }
}

pub fn neural_shaders_available() -> bool {
    ravu_dir().is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_define_with_comment() {
        let src = "#define SHARPNESS 0.25 // Amount of sharpening. 0.0 to 1.0.\n";
        let result = replace_define(src, "SHARPNESS", "0.75");
        assert!(result.contains("#define SHARPNESS 0.75 // Amount of sharpening"));
    }

    #[test]
    fn replace_define_without_comment() {
        let src = "#define FOO 42\n";
        let result = replace_define(src, "FOO", "99");
        assert!(result.contains("#define FOO 99\n"));
    }

    #[test]
    fn replace_define_preserves_other_lines() {
        let src = "// header\n#define X 1\n#define Y 2\n// footer\n";
        let result = replace_define(src, "X", "100");
        assert!(result.contains("#define X 100"));
        assert!(result.contains("#define Y 2"));
        assert!(result.contains("// header"));
        assert!(result.contains("// footer"));
    }

    #[test]
    fn neural_upscaler_filename_ravu_r4() {
        assert_eq!(
            neural_upscaler_filename("ravu-r4"),
            Some("ravu-lite-ar-r4.hook")
        );
    }

    #[test]
    fn neural_upscaler_filename_unknown_returns_none() {
        assert_eq!(neural_upscaler_filename(""), None);
        assert_eq!(neural_upscaler_filename("unknown"), None);
        assert_eq!(neural_upscaler_filename("ravu-r3"), None);
        assert_eq!(neural_upscaler_filename("ravu-r2"), None);
        assert_eq!(neural_upscaler_filename("artcnn-c4f32"), None);
    }
}
