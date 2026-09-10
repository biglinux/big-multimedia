use std::collections::HashMap;

use log::{error, info};

use crate::player::mpv_backend::{AudioMetadata, MpvBackend};

use super::chain::{LOUDNORM_MUSIC, build_superequalizer, crossfade_volume, should_crossfade};

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

    pub fn set_exclusive_mode(&self, enabled: bool) {
        self.with_mpv(|mpv| {
            let _ = mpv.set_property("audio-exclusive", enabled);
        });
        info!("Exclusive audio mode: {enabled}");
    }

    pub fn set_stereo_downmix(&self, enabled: bool) {
        self.with_mpv(|mpv| {
            if enabled {
                let _ = mpv.set_property("audio-channels", "stereo");
                let _ = mpv.set_property("audio-normalize-downmix", true);
            } else {
                let _ = mpv.set_property("audio-channels", "auto");
                let _ = mpv.set_property("audio-normalize-downmix", false);
            }
        });
    }

    pub fn set_music_equalizer(&self, bands: &[f64; 10], loudnorm: bool) {
        let mut filters = Vec::new();

        if let Some(eq) = build_superequalizer(bands) {
            filters.push(eq);
        }

        if loudnorm {
            filters.push(LOUDNORM_MUSIC.into());
        }

        let af_value = if filters.is_empty() {
            String::new()
        } else {
            format!("lavfi=[{}]", filters.join(","))
        };

        self.with_mpv(|mpv| {
            if let Err(error) = mpv.set_property("af", &*af_value) {
                error!("Failed to set music audio filters: {error}");
            }
        });
    }

    pub fn check_crossfade(&self, crossfade_secs: f64) -> bool {
        let pos = self.get_position();
        let dur = self.get_duration();
        should_crossfade(pos, dur, crossfade_secs)
    }

    pub fn apply_crossfade_volume(&self, crossfade_secs: f64) {
        if crossfade_secs <= 0.0 {
            return;
        }
        let pos = self.get_position();
        let dur = self.get_duration();
        let remaining = dur - pos;
        let current_vol = self.get_volume();
        if let Some(faded) = crossfade_volume(current_vol, remaining, crossfade_secs) {
            self.with_mpv(|mpv| {
                let _ = mpv.set_property("volume", faded);
            });
        }
    }

    pub fn restore_volume_to(&self, volume: f64) {
        self.with_mpv(|mpv| {
            let _ = mpv.set_property("volume", volume);
        });
    }

    pub fn set_audio_device(&self, device: &str) {
        self.with_mpv(|mpv| {
            if let Err(error) = mpv.set_property("audio-device", device) {
                error!("Failed to set audio device to {device}: {error}");
            }
        });
    }

    pub fn get_current_ao(&self) -> String {
        self.with_mpv(|mpv| mpv.get_property::<String>("current-ao").unwrap_or_default())
    }

    /// Avoids LADSPA-plugin segfaults (e.g. gtcrn) by quiescing the audio
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
