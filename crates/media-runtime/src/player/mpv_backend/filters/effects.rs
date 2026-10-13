use std::sync::atomic::Ordering;

use big_audio_effects::noise_reduction::{LADSPA_FILTER_NAME, NoiseModel};
use log::debug;

use crate::player::mpv_backend::{AudioEffects, MpvBackend};

use super::chain::build_audio_filter_chain;

/// mpv label of the audio effect chain, the target of live filter commands.
const AUDIO_FILTER_LABEL: &str = "bigfx";

impl MpvBackend {
    /// Change the running denoiser's strength without rebuilding the chain.
    ///
    /// Replacing `af` discards the audio the filters hold, so a slider drag
    /// would stutter. mpv rebuilds the chain
    /// from its text on every seek, loop and file, which forgets this value:
    /// the caller must also rebuild the chain once the slider settles.
    /// Returns `false` when no denoiser took the command.
    pub fn set_noise_strength(&self, model: NoiseModel, strength: f64) -> bool {
        if !self.has_af.load(Ordering::Relaxed) {
            return false;
        }
        let value = model.attenuation_control(strength);
        self.with_mpv(|mpv| {
            mpv.command(
                "af-command",
                &[AUDIO_FILTER_LABEL, "c0", &value, LADSPA_FILTER_NAME],
            )
            .is_ok()
        })
    }

    pub fn apply_audio_effects(&self, fx: &AudioEffects) {
        let filters = build_audio_filter_chain(fx);

        self.with_mpv(|mpv| {
            if filters.is_empty() {
                if self.has_af.swap(false, Ordering::Relaxed) {
                    debug!("apply_audio_effects: af=\"\" (clearing previous filters)");
                    let _ = mpv.set_property("af", String::new());
                    let _ = mpv.command("ao-reload", &[]);
                }
            } else {
                self.has_af.store(true, Ordering::Relaxed);
                // Quoted by byte length: `[...]` quoting ends at the noise
                // chain's first pad label. The label names the filter for
                // live commands.
                let graph = filters.join(",");
                let chain = format!("@{AUDIO_FILTER_LABEL}:lavfi=graph=%{}%{graph}", graph.len());
                debug!("apply_audio_effects: af=\"{chain}\"");
                let _ = mpv.set_property("af", chain);
            }
        });
    }
}
