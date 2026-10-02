use std::collections::HashMap;

/// Parameters for building the audio filter chain.
pub struct AudioEffects<'a> {
    pub normalize: bool,
    /// Voice noise reduction strength; 0 turns it off.
    pub nr_strength: f64,
    pub nr_model: i32,
    /// Mix to mono before the denoiser.
    pub nr_mono: bool,
    pub eq_bands: &'a [(f64, f64)],
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
