//! Pure, GTK-free filter/shader value mapping for the video filters dialog.
//!
//! Holds the UI↔shader percentage conversions (ported verbatim from the legacy
//! `dialogs/filters/shaders.rs`, with their round-trip tests) and the single
//! source of truth for each single-parameter `vf` filter's mpv string. Keeping
//! the math here lets the Relm4 dialog stay a thin widget layer and lets the
//! mappings be unit-tested without a running mpv backend.

/// FSR shader sharpness (0..=2, lower = sharper) → UI percent (0..=100).
#[must_use]
pub fn fsr_shader_to_ui_pct(sharpness: f64) -> f64 {
    (1.0 - sharpness / 2.0) * 100.0
}

/// UI percent (0..=100) → FSR shader sharpness (0..=2).
#[must_use]
pub fn fsr_ui_pct_to_shader(pct: f64) -> f64 {
    2.0 * (1.0 - pct / 100.0)
}

/// CAS shader sharpening (0..=1) → UI percent (0..=100).
#[must_use]
pub fn cas_shader_to_ui_pct(sharpening: f64) -> f64 {
    sharpening * 100.0
}

/// UI percent (0..=100) → CAS shader sharpening (0..=1).
#[must_use]
pub fn cas_ui_pct_to_shader(pct: f64) -> f64 {
    pct / 100.0
}

/// `colortemperature` vf string for a Kelvin temperature.
#[must_use]
pub fn colortemp_vf(kelvin: f64) -> String {
    format!("colortemperature=temperature={kelvin:.0}")
}

/// `colorize` vf string from hue (deg), saturation (0..1), mix (0..1).
#[must_use]
pub fn colorize_vf(hue: f64, saturation: f64, mix: f64) -> String {
    format!("colorize=hue={hue:.0}:saturation={saturation:.2}:mix={mix:.2}")
}

/// `hqdn3d` denoise vf string from a single strength value.
#[must_use]
pub fn hqdn3d_vf(strength: f64) -> String {
    format!(
        "hqdn3d=luma_spatial={strength}:chroma_spatial={}:luma_tmp={}",
        strength * 0.75,
        strength * 0.5
    )
}

/// `pixelize` vf string from a block size (rounded to an integer).
#[must_use]
pub fn pixelize_vf(block: f64) -> String {
    let block = block as i32;
    format!("pixelize=width={block}:height={block}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsr_ui_roundtrip_keeps_percentage() {
        let pct = 72.0;
        let sharpness = fsr_ui_pct_to_shader(pct);
        assert!((fsr_shader_to_ui_pct(sharpness) - pct).abs() < f64::EPSILON);
    }

    #[test]
    fn fsr_zero_ui_means_max_shader_sharpness() {
        assert!((fsr_ui_pct_to_shader(0.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cas_ui_roundtrip_keeps_percentage() {
        let pct = 37.0;
        let sharpening = cas_ui_pct_to_shader(pct);
        assert!((cas_shader_to_ui_pct(sharpening) - pct).abs() < f64::EPSILON);
    }

    #[test]
    fn cas_full_ui_means_full_shader_strength() {
        assert!((cas_ui_pct_to_shader(100.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn colortemp_rounds_kelvin() {
        assert_eq!(colortemp_vf(6500.4), "colortemperature=temperature=6500");
    }

    #[test]
    fn colorize_formats_all_params() {
        assert_eq!(
            colorize_vf(30.0, 0.5, 0.75),
            "colorize=hue=30:saturation=0.50:mix=0.75"
        );
    }

    #[test]
    fn hqdn3d_derives_chroma_and_tmp_from_strength() {
        assert_eq!(
            hqdn3d_vf(4.0),
            "hqdn3d=luma_spatial=4:chroma_spatial=3:luma_tmp=2"
        );
    }

    #[test]
    fn pixelize_truncates_block_size() {
        assert_eq!(pixelize_vf(16.9), "pixelize=width=16:height=16");
    }
}
