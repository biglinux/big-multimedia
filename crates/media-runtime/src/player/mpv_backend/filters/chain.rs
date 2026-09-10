use big_audio_effects::dynamics::{ffmpeg_acompressor_filter, ffmpeg_agate_filter};
use big_audio_effects::noise_reduction::{NoiseModel, mpv_ladspa_filter_for_model};

use crate::player::mpv_backend::AudioEffects;
use big_audio_effects::noise_reduction::GtcrnControls;

/// EBU R128 streaming target (-16 LUFS, TP=-1.5). Use for live playback effects.
const LOUDNORM_STREAMING: &str = "loudnorm=I=-16:TP=-1.5:LRA=11";
/// Music target (-14 LUFS, TP=-1). Slightly louder for focused music listening mode.
pub(super) const LOUDNORM_MUSIC: &str = "loudnorm=I=-14:TP=-1:LRA=11";

/// Build the lavfi audio filter chain string from AudioEffects.
/// Returns individual filter entries (to be joined with `,` inside `lavfi=[...]`).
pub fn build_audio_filter_chain(fx: &AudioEffects) -> Vec<String> {
    let mut filters = Vec::new();

    if fx.hpf_enabled && fx.hpf_frequency > 0.0 {
        filters.push(format!("highpass=f={:.0}:poles=2", fx.hpf_frequency));
    }

    if fx.compressor_enabled {
        filters.push(ffmpeg_acompressor_filter(fx.compressor_intensity));
    }

    if fx.normalize {
        filters.push(LOUDNORM_STREAMING.to_string());
    }

    if fx.nr_enabled {
        filters.push("aresample=48000".to_string());
        let model = NoiseModel::from_i32_or_default(fx.nr_model);
        let controls = GtcrnControls::player(
            fx.nr_strength,
            model as i32,
            fx.nr_model_blend,
            fx.nr_voice_recovery,
        );
        if let Some(filter) = mpv_ladspa_filter_for_model(model, &controls) {
            filters.push(filter);
        } else {
            let _ = filters.pop();
        }
    }

    if fx.gate_enabled {
        filters.push(ffmpeg_agate_filter(fx.gate_intensity));
    }

    for &(freq, gain) in fx.eq_bands {
        if gain.abs() > 0.1 {
            filters.push(format!("equalizer=f={freq}:t=o:w=1.0:g={gain}"));
        }
    }

    filters
}

/// Check if crossfade should begin given position, duration, window.
pub(super) fn should_crossfade(pos: f64, dur: f64, window: f64) -> bool {
    if window <= 0.0 || dur <= 0.0 {
        return false;
    }
    let remaining = dur - pos;
    remaining <= window && remaining > 0.0
}

/// Calculate faded volume during crossfade.
/// Returns `None` if not in crossfade zone.
pub(super) fn crossfade_volume(current_vol: f64, remaining: f64, window: f64) -> Option<f64> {
    if window <= 0.0 || remaining <= 0.0 || remaining > window {
        return None;
    }
    let fade_factor = remaining / window;
    Some(current_vol * fade_factor)
}

/// Build superequalizer filter string from 10-band gains.
/// Returns `None` if all bands are near zero.
pub(super) fn build_superequalizer(bands: &[f64; 10]) -> Option<String> {
    if bands.iter().all(|band| band.abs() <= 0.01) {
        return None;
    }
    Some(format!(
        "superequalizer=1b={:.1}:2b={:.1}:3b={:.1}:4b={:.1}:5b={:.1}:\
         6b={:.1}:7b={:.1}:8b={:.1}:9b={:.1}:10b={:.1}",
        bands[0],
        bands[1],
        bands[2],
        bands[3],
        bands[4],
        bands[5],
        bands[6],
        bands[7],
        bands[8],
        bands[9]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crossfade_disabled_when_window_zero() {
        assert!(!should_crossfade(50.0, 100.0, 0.0));
    }

    #[test]
    fn test_crossfade_disabled_when_window_negative() {
        assert!(!should_crossfade(50.0, 100.0, -5.0));
    }

    #[test]
    fn test_crossfade_disabled_when_duration_zero() {
        assert!(!should_crossfade(0.0, 0.0, 5.0));
    }

    #[test]
    fn test_crossfade_not_in_window() {
        assert!(!should_crossfade(50.0, 100.0, 10.0));
    }

    #[test]
    fn test_crossfade_at_boundary() {
        assert!(should_crossfade(90.0, 100.0, 10.0));
    }

    #[test]
    fn test_crossfade_inside_window() {
        assert!(should_crossfade(95.0, 100.0, 10.0));
    }

    #[test]
    fn test_crossfade_at_end_pos_equals_dur() {
        assert!(!should_crossfade(100.0, 100.0, 10.0));
    }

    #[test]
    fn test_crossfade_past_end() {
        assert!(!should_crossfade(105.0, 100.0, 10.0));
    }

    #[test]
    fn test_crossfade_volume_window_zero() {
        assert_eq!(crossfade_volume(100.0, 5.0, 0.0), None);
    }

    #[test]
    fn test_crossfade_volume_remaining_zero() {
        assert_eq!(crossfade_volume(100.0, 0.0, 10.0), None);
    }

    #[test]
    fn test_crossfade_volume_not_in_window() {
        assert_eq!(crossfade_volume(100.0, 15.0, 10.0), None);
    }

    #[test]
    fn test_crossfade_volume_halfway() {
        let volume = crossfade_volume(100.0, 5.0, 10.0).unwrap();
        assert!((volume - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_crossfade_volume_near_end() {
        let volume = crossfade_volume(100.0, 1.0, 10.0).unwrap();
        assert!((volume - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_crossfade_volume_at_boundary() {
        let volume = crossfade_volume(80.0, 10.0, 10.0).unwrap();
        assert!((volume - 80.0).abs() < 0.01);
    }

    #[test]
    fn test_supereq_all_zero_returns_none() {
        assert!(build_superequalizer(&[0.0; 10]).is_none());
    }

    #[test]
    fn test_supereq_all_near_zero_returns_none() {
        assert!(build_superequalizer(&[0.005; 10]).is_none());
    }

    #[test]
    fn test_supereq_with_gains() {
        let bands = [1.0, -2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 5.0];
        let result = build_superequalizer(&bands).unwrap();
        assert!(result.starts_with("superequalizer="));
        assert!(result.contains("1b=1.0"));
        assert!(result.contains("2b=-2.0"));
        assert!(result.contains("3b=3.0"));
        assert!(result.contains("10b=5.0"));
    }

    #[test]
    fn test_filter_chain_no_effects() {
        let fx = AudioEffects {
            normalize: false,
            nr_enabled: false,
            nr_strength: 0.5,
            nr_model: 0,
            nr_model_blend: false,
            nr_voice_recovery: 0.0,
            gate_enabled: false,
            gate_intensity: 0.0,
            hpf_enabled: false,
            hpf_frequency: 0.0,
            eq_bands: &[],
            compressor_enabled: false,
            compressor_intensity: 0.0,
        };
        assert!(build_audio_filter_chain(&fx).is_empty());
    }

    #[test]
    fn test_filter_chain_normalize_only() {
        let fx = AudioEffects {
            normalize: true,
            nr_enabled: false,
            nr_strength: 0.5,
            nr_model: 0,
            nr_model_blend: false,
            nr_voice_recovery: 0.0,
            gate_enabled: false,
            gate_intensity: 0.0,
            hpf_enabled: false,
            hpf_frequency: 0.0,
            eq_bands: &[],
            compressor_enabled: false,
            compressor_intensity: 0.0,
        };
        let chain = build_audio_filter_chain(&fx);
        assert_eq!(chain.len(), 1);
        assert!(chain[0].contains("loudnorm"));
    }

    #[test]
    fn test_filter_chain_hpf_only() {
        let fx = AudioEffects {
            normalize: false,
            nr_enabled: false,
            nr_strength: 0.5,
            nr_model: 0,
            nr_model_blend: false,
            nr_voice_recovery: 0.0,
            gate_enabled: false,
            gate_intensity: 0.0,
            hpf_enabled: true,
            hpf_frequency: 120.0,
            eq_bands: &[],
            compressor_enabled: false,
            compressor_intensity: 0.0,
        };
        let chain = build_audio_filter_chain(&fx);
        assert_eq!(chain.len(), 1);
        assert!(chain[0].contains("highpass=f=120"));
    }

    #[test]
    fn test_filter_chain_gate_only() {
        let fx = AudioEffects {
            normalize: false,
            nr_enabled: false,
            nr_strength: 0.5,
            nr_model: 0,
            nr_model_blend: false,
            nr_voice_recovery: 0.0,
            gate_enabled: true,
            gate_intensity: 0.7,
            hpf_enabled: false,
            hpf_frequency: 0.0,
            eq_bands: &[],
            compressor_enabled: false,
            compressor_intensity: 0.0,
        };
        let chain = build_audio_filter_chain(&fx);
        assert_eq!(chain.len(), 1);
        assert!(chain[0].contains("agate=threshold="));
        assert!(chain[0].contains(":range="));
    }

    #[test]
    fn test_filter_chain_eq_only() {
        let bands = [(100.0, 3.0), (1000.0, -2.0), (5000.0, 0.05)];
        let fx = AudioEffects {
            normalize: false,
            nr_enabled: false,
            nr_strength: 0.5,
            nr_model: 0,
            nr_model_blend: false,
            nr_voice_recovery: 0.0,
            gate_enabled: false,
            gate_intensity: 0.0,
            hpf_enabled: false,
            hpf_frequency: 0.0,
            eq_bands: &bands,
            compressor_enabled: false,
            compressor_intensity: 0.0,
        };
        let chain = build_audio_filter_chain(&fx);
        assert_eq!(chain.len(), 2);
        assert!(chain[0].contains("f=100"));
        assert!(chain[1].contains("f=1000"));
    }

    #[test]
    fn test_filter_chain_eq_all_below_threshold() {
        let bands = [(100.0, 0.05), (200.0, -0.09)];
        let fx = AudioEffects {
            normalize: false,
            nr_enabled: false,
            nr_strength: 0.5,
            nr_model: 0,
            nr_model_blend: false,
            nr_voice_recovery: 0.0,
            gate_enabled: false,
            gate_intensity: 0.0,
            hpf_enabled: false,
            hpf_frequency: 0.0,
            eq_bands: &bands,
            compressor_enabled: false,
            compressor_intensity: 0.0,
        };
        assert!(build_audio_filter_chain(&fx).is_empty());
    }
}
