use std::collections::HashMap;

/// Parameters for building the audio filter chain.
pub struct AudioEffects<'a> {
    pub normalize: bool,
    pub nr_enabled: bool,
    pub nr_strength: f64,
    pub nr_model: i32,
    /// Mix to mono before the denoiser.
    pub nr_mono: bool,
    pub gate_enabled: bool,
    pub gate_intensity: f64,
    pub hpf_enabled: bool,
    pub hpf_frequency: f64,
    pub eq_bands: &'a [(f64, f64)],
    pub compressor_enabled: bool,
    pub compressor_intensity: f64,
}

/// Owned mirror of [`AudioEffects`] for passing the effect set through
/// message channels (the borrowed `eq_bands` slice of `AudioEffects` can't
/// cross a `'static` message boundary). Build it from the same values the UI
/// reads from its effect controls, then borrow via [`Self::as_effects`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioEffectsSnapshot {
    /// See [`AudioEffects::normalize`].
    pub normalize: bool,
    /// See [`AudioEffects::nr_enabled`].
    pub nr_enabled: bool,
    /// See [`AudioEffects::nr_strength`].
    pub nr_strength: f64,
    /// See [`AudioEffects::nr_model`].
    pub nr_model: i32,
    /// See [`AudioEffects::nr_mono`].
    pub nr_mono: bool,
    /// See [`AudioEffects::gate_enabled`].
    pub gate_enabled: bool,
    /// See [`AudioEffects::gate_intensity`].
    pub gate_intensity: f64,
    /// See [`AudioEffects::hpf_enabled`].
    pub hpf_enabled: bool,
    /// See [`AudioEffects::hpf_frequency`].
    pub hpf_frequency: f64,
    /// Owned `(frequency_hz, gain_db)` band pairs; empty when EQ is off.
    pub eq_bands: Vec<(f64, f64)>,
    /// See [`AudioEffects::compressor_enabled`].
    pub compressor_enabled: bool,
    /// See [`AudioEffects::compressor_intensity`].
    pub compressor_intensity: f64,
}

impl AudioEffectsSnapshot {
    /// Borrow this snapshot as an [`AudioEffects`] for the filter-chain builder.
    #[must_use]
    pub fn as_effects(&self) -> AudioEffects<'_> {
        AudioEffects {
            normalize: self.normalize,
            nr_enabled: self.nr_enabled,
            nr_strength: self.nr_strength,
            nr_model: self.nr_model,
            nr_mono: self.nr_mono,
            gate_enabled: self.gate_enabled,
            gate_intensity: self.gate_intensity,
            hpf_enabled: self.hpf_enabled,
            hpf_frequency: self.hpf_frequency,
            eq_bands: &self.eq_bands,
            compressor_enabled: self.compressor_enabled,
            compressor_intensity: self.compressor_intensity,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub id: i64,
    pub track_type: String,
    pub title: String,
    pub lang: String,
    pub selected: bool,
    pub codec: String,
    pub external: bool,
}

/// Live audio information from MPV (for music mode display).
#[derive(Debug, Clone, Default)]
pub struct AudioMetadata {
    pub codec: String,
    pub sample_rate: i64,
    pub channels: String,
    pub channel_count: i64,
    pub bitrate: i64,
    pub format: String,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    PositionChanged(f64),
    DurationChanged(f64),
    PauseChanged(bool),
    /// Native discontinuity after SEEK followed by PLAYBACK_RESTART.
    Seeked(f64),
    RateChanged(f64),
    VolumeChanged(f64),
    MuteChanged(bool),
    FileLoaded(String),
    /// End file with reason code (Eof=0, Stop=2, Quit=3, Error=4, Redirect=5)
    EndFile(u32),
    TracksChanged(Vec<TrackMetadata>),
    ChaptersChanged(Vec<(f64, String)>),
    MetadataChanged(HashMap<String, String>),
    /// Buffering state change — true when mpv is paused waiting for cache data
    /// (radio streams / slow network). Derived from `paused-for-cache` property.
    Buffering(bool),
    /// Internal native receipt, consumed by the backend before UI delivery.
    AsyncCommandReply {
        id: u64,
        error: i32,
    },
    ScreenshotFinished {
        id: u64,
        result: super::ScreenshotResult,
    },
    ScreenshotTimedOut {
        id: u64,
    },
    RenderUpdate,
    Shutdown,
}

/// Demuxer-cache sizing policy for the audio-only backend. Each profile is
/// a `(forward, back)` pair in `mpv` `demuxer-max-bytes` / `demuxer-max-back-bytes`
/// string form ("NMiB"). Switching profile at runtime is cheap — mpv
/// truncates the queue in-place to the new limit.
///
/// Memory impact for a typical podcast session (1-hour MP3, ~100 MB):
/// * `Music`    → 30 + 10 MiB = ~40 MiB cap (gapless album playback)
/// * `Podcast`  → 8 + 2 MiB = ~10 MiB cap (single long stream, no gapless need)
/// * `Radio`    → 64 + 4 MiB = ~68 MiB cap (network jitter resilience)
///
/// Picking `Podcast` over the previous unconditional 150+150 MiB budget cuts
/// ~120-160 MB RSS during long-form playback. See R7 in
/// `biglinux-body-tracker/docs/RESILIENCE.md` for the broader argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCacheProfile {
    Music,
    Podcast,
    Radio,
}

impl AudioCacheProfile {
    #[must_use]
    pub fn forward_bytes(self) -> &'static str {
        match self {
            Self::Music => "30MiB",
            Self::Podcast => "8MiB",
            Self::Radio => "64MiB",
        }
    }

    #[must_use]
    pub fn back_bytes(self) -> &'static str {
        match self {
            Self::Music => "10MiB",
            Self::Podcast => "2MiB",
            Self::Radio => "4MiB",
        }
    }
}
