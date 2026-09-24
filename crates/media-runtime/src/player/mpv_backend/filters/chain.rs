use big_audio_effects::dynamics::{ffmpeg_acompressor_filter, ffmpeg_agate_filter};
use big_audio_effects::noise_reduction::{NoiseModel, mpv_ladspa_filter_for_model};

use crate::player::mpv_backend::AudioEffects;
use big_audio_effects::noise_reduction::GtcrnControls;

/// EBU R128 streaming target (-16 LUFS, TP=-1.5). Use for live playback effects.
const LOUDNORM_STREAMING: &str = "loudnorm=I=-16:TP=-1.5:LRA=11";

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

#[cfg(test)]
mod tests {
    use super::*;

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
