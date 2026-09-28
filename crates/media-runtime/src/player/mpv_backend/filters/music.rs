use std::collections::HashMap;

use log::{error, info};

use crate::player::mpv_backend::{AudioMetadata, MpvBackend};

impl MpvBackend {
    /// `mode`: `"no"` | `"track"` | `"album"`.
    pub fn set_replaygain(&self, mode: &str) {
        let mode = match mode {
            "track" | "album" => mode,
            _ => "no",
        };
        self.with_mpv(|mpv| {
            if let Err(error) = mpv.set_property("replaygain", mode) {
                error!("Failed to set replaygain to {mode}: {error}");
            } else {
                info!("ReplayGain set to: {mode}");
            }
        });
    }

    pub fn set_replaygain_preamp(&self, db: f64) {
        let db = db.clamp(-15.0, 15.0);
        self.with_mpv(|mpv| {
            let _ = mpv.set_property("replaygain-preamp", db);
        });
    }

    pub fn set_gapless(&self, enabled: bool) {
        let mode = if enabled { "weak" } else { "no" };
        self.with_mpv(|mpv| {
            let _ = mpv.set_property("gapless-audio", mode);
        });
        info!("Gapless audio: {mode}");
    }

    pub fn get_audio_metadata(&self) -> AudioMetadata {
        self.with_mpv(|mpv| AudioMetadata {
            codec: mpv
                .get_property::<String>("audio-codec-name")
                .unwrap_or_default(),
            sample_rate: mpv
                .get_property::<i64>("audio-params/samplerate")
                .unwrap_or(0),
            channels: mpv
                .get_property::<String>("audio-params/channels")
                .unwrap_or_default(),
            channel_count: mpv
                .get_property::<i64>("audio-params/channel-count")
                .unwrap_or(0),
            bitrate: mpv.get_property::<i64>("audio-bitrate").unwrap_or(0),
            format: mpv
                .get_property::<String>("audio-params/format")
                .unwrap_or_default(),
        })
    }

    pub fn get_metadata(&self) -> HashMap<String, String> {
        self.with_mpv(|mpv| {
            let mut meta = HashMap::new();
            for key in &["title", "artist", "album", "genre", "date", "track"] {
                let prop = format!("metadata/by-key/{key}");
                if let Ok(value) = mpv.get_property::<String>(&prop) {
                    meta.insert(key.to_string(), value);
                }
            }
            meta
        })
    }

    pub fn get_current_ao(&self) -> String {
        self.with_mpv(|mpv| mpv.get_property::<String>("current-ao").unwrap_or_default())
    }

    /// Avoids LADSPA-plugin crashes at teardown by quiescing the audio
    /// pipeline before mpv teardown.
    pub fn shutdown_audio(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.with_mpv(|mpv| {
            let _ = mpv.set_property("pause", true);
            let _ = mpv.set_property("af", String::new());
            let _ = mpv.command("stop", &[]);
            let _ = mpv.command("quit", &[]);
        });
    }
}
