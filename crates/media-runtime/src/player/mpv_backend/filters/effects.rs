use std::sync::atomic::Ordering;

use log::debug;

use crate::player::mpv_backend::{AudioEffects, MpvBackend};

use super::chain::build_audio_filter_chain;

impl MpvBackend {
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
                let chain = format!("lavfi=[{}]", filters.join(","));
                debug!("apply_audio_effects: af=\"{chain}\"");
                let _ = mpv.set_property("af", chain);
            }
        });
    }
}
