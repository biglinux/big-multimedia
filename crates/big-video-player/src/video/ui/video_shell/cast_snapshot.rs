use super::*;
use crate::cast::transcode::TranscodeFilters;

const EQ_BAND_FREQUENCIES: [f32; 10] = [
    31.0, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

pub(super) fn build_cast_filter_snapshot(
    mpv: &Rc<RefCell<Option<Arc<MpvBackend>>>>,
    settings: &Settings,
    transform_state: &media_adjust::VideoTransformState,
) -> TranscodeFilters {
    let mut filters = TranscodeFilters::default();

    let noise_strength = settings.get_double(keys::NOISE_STRENGTH).clamp(0.0, 1.0) as f32;
    filters.noise_strength = noise_strength;
    filters.voice_enhance =
        noise_strength > 0.0 && crate::shared::audio_effects::has_noise_reduction_backend();
    filters.eq_bands = eq_bands_from_settings(settings);
    filters.hflip = transform_state.hflip();
    filters.vflip = transform_state.vflip();
    filters.sharpness = shader_sharpness_from_settings(settings);

    if let Some(mpv) = mpv.borrow().as_ref() {
        filters.brightness = eq_percent_to_offset(mpv.get_brightness());
        filters.contrast = eq_percent_to_multiplier(mpv.get_contrast(), 0.0, 2.0);
        filters.saturation = eq_percent_to_multiplier(mpv.get_saturation(), 0.0, 3.0);
        filters.sharpness = filters
            .sharpness
            .max(mpv.get_property_f64("sharpen").clamp(0.0, 2.0) as f32);
        filters.volume = (mpv.get_volume() / 100.0).clamp(0.0, 4.0) as f32;
        filters.speed = mpv.get_speed().clamp(0.25, 4.0) as f32;
        filters.rotation = normalize_cast_rotation(mpv.get_video_rotate());
    }

    filters
}

fn shader_sharpness_from_settings(settings: &Settings) -> f32 {
    match settings.get_string(keys::BUILTIN_UPSCALER).as_str() {
        "fsr" => (2.0 - settings.get_double(keys::SHADER_FSR_SHARPNESS)).clamp(0.0, 2.0) as f32,
        "cas" => (settings.get_double(keys::SHADER_CAS_SHARPENING) * 2.0).clamp(0.0, 2.0) as f32,
        _ => 0.0,
    }
}

fn eq_bands_from_settings(settings: &Settings) -> Vec<(f32, f32)> {
    let gains = crate::shared::audio_effects::parse_eq_bands(&settings.get_string(keys::EQ_BANDS));
    EQ_BAND_FREQUENCIES
        .iter()
        .copied()
        .zip(gains)
        .filter_map(|(frequency, gain)| {
            let gain = gain as f32;
            (gain.abs() > f32::EPSILON).then_some((frequency, gain))
        })
        .collect()
}

fn eq_percent_to_offset(value: i64) -> f32 {
    (value as f32 / 100.0).clamp(-1.0, 1.0)
}

fn eq_percent_to_multiplier(value: i64, lower: f32, upper: f32) -> f32 {
    (1.0 + value as f32 / 100.0).clamp(lower, upper)
}

fn normalize_cast_rotation(degrees: i64) -> i32 {
    match degrees.rem_euclid(360) {
        1..=134 => 90,
        135..=224 => 180,
        225..=314 => 270,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{eq_percent_to_multiplier, eq_percent_to_offset, normalize_cast_rotation};

    #[test]
    fn percent_offset_maps_mpv_equalizer_range() {
        assert_eq!(eq_percent_to_offset(-100), -1.0);
        assert_eq!(eq_percent_to_offset(0), 0.0);
        assert_eq!(eq_percent_to_offset(100), 1.0);
    }

    #[test]
    fn percent_multiplier_maps_neutral_to_one() {
        assert_eq!(eq_percent_to_multiplier(0, 0.0, 2.0), 1.0);
        assert_eq!(eq_percent_to_multiplier(-100, 0.0, 2.0), 0.0);
        assert_eq!(eq_percent_to_multiplier(100, 0.0, 2.0), 2.0);
    }

    #[test]
    fn rotation_snapshot_uses_quarter_turns() {
        assert_eq!(normalize_cast_rotation(0), 0);
        assert_eq!(normalize_cast_rotation(90), 90);
        assert_eq!(normalize_cast_rotation(-90), 270);
        assert_eq!(normalize_cast_rotation(181), 180);
    }
}
