use big_audio_effects::noise_reduction::{NoiseModel, lavfi_chain};

use crate::player::mpv_backend::AudioEffects;

/// EBU R128 streaming target (-16 LUFS, TP=-1.5). Use for live playback effects.
const LOUDNORM_STREAMING: &str = "loudnorm=I=-16:TP=-1.5:LRA=11";

/// Build the lavfi audio filter chain string from AudioEffects.
/// Returns individual filter entries, joined with `,` into one lavfi graph.
pub fn build_audio_filter_chain(fx: &AudioEffects) -> Vec<String> {
    let mut filters = Vec::new();

    if fx.normalize {
        filters.push(LOUDNORM_STREAMING.to_string());
    }

    if fx.nr_strength > 0.0 {
        // A missing plugin would fail the whole graph, silencing playback.
        let model = NoiseModel::from_i32_or_default(fx.nr_model);
        if model.plugin_available() {
            filters.push(lavfi_chain(
                model,
                fx.nr_strength,
                fx.nr_mono,
                fx.nr_voice_only,
            ));
        }
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

    fn effects(normalize: bool, nr_strength: f64, eq_bands: &[(f64, f64)]) -> AudioEffects<'_> {
        AudioEffects {
            normalize,
            nr_strength,
            nr_model: NoiseModel::DpdfNet2.index() as i32,
            nr_mono: false,
            nr_voice_only: None,
            eq_bands,
        }
    }

    #[test]
    fn noise_reduction_requires_installed_chain() {
        let model = NoiseModel::DpdfNet2;
        let chain = build_audio_filter_chain(&effects(false, 0.5, &[]));
        if model.plugin_available() {
            assert_eq!(chain, [lavfi_chain(model, 0.5, false, None)]);
        } else {
            assert!(chain.is_empty(), "{chain:?}");
        }
    }

    #[test]
    fn no_effects_build_empty_chain() {
        assert!(build_audio_filter_chain(&effects(false, 0.0, &[])).is_empty());
    }

    #[test]
    fn normalize_adds_loudnorm() {
        let chain = build_audio_filter_chain(&effects(true, 0.0, &[]));
        assert_eq!(chain.len(), 1);
        assert!(chain[0].contains("loudnorm"));
    }

    #[test]
    fn eq_skips_flat_bands() {
        let bands = [(100.0, 3.0), (1000.0, -2.0), (5000.0, 0.05)];
        let chain = build_audio_filter_chain(&effects(false, 0.0, &bands));
        assert_eq!(chain.len(), 2);
        assert!(chain[0].contains("f=100"));
        assert!(chain[1].contains("f=1000"));
    }

    #[test]
    fn flat_eq_builds_empty_chain() {
        let bands = [(100.0, 0.05), (200.0, -0.09)];
        assert!(build_audio_filter_chain(&effects(false, 0.0, &bands)).is_empty());
    }
}
