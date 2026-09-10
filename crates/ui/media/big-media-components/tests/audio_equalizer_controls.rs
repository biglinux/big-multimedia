// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

use big_audio_effects::equalizer::VOICE_EQ_PRESET_IDS;
use big_media_components::audio::equalizer_controls::{
    BigAudioEqualizerLabels, BigAudioEqualizerSpec, BigAudioEqualizerVerticalSpec,
    compact_frequency_label, frequency_label, parse_clamped_band_gains,
};

#[test]
fn frequency_label_uses_khz_for_large_values() {
    assert_eq!(frequency_label(60), "60 Hz");
    assert_eq!(frequency_label(1_000), "1 kHz");
    assert_eq!(frequency_label(16_000), "16 kHz");
}

#[test]
fn resolved_reports_invalid_preset_band_lengths() {
    let spec = BigAudioEqualizerSpec::new(
        BigAudioEqualizerLabels::english(),
        [60, 170, 310],
        ["Flat"],
        [vec![0.0, 0.0]],
    );

    assert_eq!(spec.resolved().band_count, 3);
    assert_eq!(spec.resolved().preset_count, 1);
    assert!(!spec.resolved().preset_rows_match_bands);
}

#[test]
fn layout_builders_clamp_negative_values() {
    let spec = BigAudioEqualizerSpec::new(
        BigAudioEqualizerLabels::english(),
        [60],
        ["Flat"],
        [vec![0.0]],
    )
    .preset_layout(-1, -2)
    .band_layout(-1, -2, -3, -4, -5);

    assert_eq!(spec.preset_row_spacing, 0);
    assert_eq!(spec.preset_margin_bottom, 0);
    assert_eq!(spec.band_title_width_chars, Some(1));
    assert_eq!(spec.band_value_width_chars, Some(1));
    assert_eq!(spec.band_spacing, Some(0));
    assert_eq!(spec.band_height_request, Some(1));
    assert_eq!(spec.band_margin_y, 0);
}

#[test]
fn vertical_voice_spec_uses_canonical_voice_preset_shape() {
    let spec = BigAudioEqualizerVerticalSpec::voice("voice_boost", [0.0; 10]);
    let resolved = spec.resolved();

    assert_eq!(resolved.band_count, 10);
    assert_eq!(resolved.preset_count, VOICE_EQ_PRESET_IDS.len());
    assert!(resolved.preset_labels_match_ids);
    assert!(resolved.preset_rows_match_bands);
    assert!(resolved.initial_bands_match);
}

#[test]
fn vertical_spec_clamps_gain_and_band_size() {
    let spec = BigAudioEqualizerVerticalSpec::voice("flat", [0.0; 10])
        .gain_range(12.0, -12.0)
        .band_size(0, -1);

    assert_eq!(spec.min_gain_db, -12.0);
    assert_eq!(spec.max_gain_db, 12.0);
    assert_eq!(spec.band_width_request, 1);
    assert_eq!(spec.band_height_request, 1);
}

#[test]
fn compact_frequency_label_uses_short_k_suffix() {
    assert_eq!(compact_frequency_label(31), "31");
    assert_eq!(compact_frequency_label(1_000), "1k");
    assert_eq!(compact_frequency_label(16_000), "16k");
}

#[test]
fn parse_clamped_band_gains_pads_and_clamps() {
    let gains = parse_clamped_band_gains::<4>("100;-100;3;bad;5", -12.0, 12.0);
    assert_eq!(gains, [12.0, -12.0, 3.0, 5.0]);
}

#[test]
fn parse_clamped_band_gains_ignores_extra_values() {
    let gains = parse_clamped_band_gains::<2>("1;2;3;4", -12.0, 12.0);
    assert_eq!(gains, [1.0, 2.0]);
}
