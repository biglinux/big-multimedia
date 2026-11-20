use std::collections::HashMap;

/// Parameters for building the audio filter chain.
pub struct AudioEffects<'a> {
    pub normalize: bool,
    /// Voice noise reduction strength; 0 turns it off.
    pub nr_strength: f64,
    pub nr_model: i32,
    /// Mix to mono before the denoiser.
    pub nr_mono: bool,
    /// Keep only speech, at this voice sensitivity (1 to 5).
    pub nr_voice_only: Option<u8>,
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
    EndFile(EndReason),
    TracksChanged(Vec<TrackMetadata>),
    ChaptersChanged(Vec<(f64, String)>),
    MetadataChanged(HashMap<String, String>),
    /// Buffering state change: true when mpv is paused waiting for cache data
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

/// Why mpv stopped playing a file, from libmpv's `mpv_end_file_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// The end of the file was reached.
    Eof,
    /// Stopped from outside, such as by loading another file.
    Stop,
    /// Stopped by the quit command or player shutdown.
    Quit,
    /// The file could not be played.
    Error,
    /// The audio output could not start; the file itself may be fine.
    AudioOutputFailed,
    /// The file was a playlist and its entries took its place.
    Redirect,
    /// A reason newer than this libmpv binding.
    Unknown(u32),
}

impl EndReason {
    /// What to tell the person when the audio output, not the file, failed.
    pub fn audio_output_failed_notice() -> String {
        crate::i18n::i18n(
            "Sound could not start on the audio output. Check that your speakers or headphones are connected, or choose another Audio Output in Preferences and reopen the player.",
        )
    }

    pub(super) fn from_mpv(reason: mpv_sys::mpv_end_file_reason) -> Self {
        match reason {
            mpv_sys::mpv_end_file_reason_MPV_END_FILE_REASON_EOF => Self::Eof,
            mpv_sys::mpv_end_file_reason_MPV_END_FILE_REASON_STOP => Self::Stop,
            mpv_sys::mpv_end_file_reason_MPV_END_FILE_REASON_QUIT => Self::Quit,
            mpv_sys::mpv_end_file_reason_MPV_END_FILE_REASON_ERROR => Self::Error,
            mpv_sys::mpv_end_file_reason_MPV_END_FILE_REASON_REDIRECT => Self::Redirect,
            other => Self::Unknown(other),
        }
    }

    /// From a whole `mpv_event_end_file`, whose error code outranks its reason.
    pub(super) fn from_end_file(
        reason: mpv_sys::mpv_end_file_reason,
        error: std::os::raw::c_int,
    ) -> Self {
        match error {
            mpv_sys::mpv_error_MPV_ERROR_AO_INIT_FAILED => Self::AudioOutputFailed,
            error if error < 0 => Self::Error,
            _ => Self::from_mpv(reason),
        }
    }
}

/// Who named a load target. mpv's guard against unsafe playlist entries does
/// not cover loads through its client API, so a list's targets must pass
/// [`big_os_kit::url::is_list_target`] at the load too.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TargetSource {
    /// The person typed, picked or opened it.
    #[default]
    Person,
    /// A feed, an IPTV list or a playlist file named it.
    List(big_os_kit::url::ListOrigin),
}

/// Demuxer-cache sizing policy for the audio-only backend. Each profile is
/// a `(forward, back)` pair in `mpv` `demuxer-max-bytes` / `demuxer-max-back-bytes`
/// string form ("NMiB"). Switching profile at runtime is cheap: mpv
/// truncates the queue in-place to the new limit.
///
/// * `Music`: enough lookahead for gapless album playback.
/// * `Podcast`: one long stream with no gapless need, the smallest cache.
/// * `Radio`: the largest forward window, against network jitter.
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

#[cfg(test)]
mod tests {
    use super::EndReason;

    /// Values from `enum mpv_end_file_reason` in libmpv's client.h.
    #[test]
    fn end_reasons_follow_client_header() {
        for (raw, reason) in [
            (0, EndReason::Eof),
            (1, EndReason::Unknown(1)),
            (2, EndReason::Stop),
            (3, EndReason::Quit),
            (4, EndReason::Error),
            (5, EndReason::Redirect),
            (6, EndReason::Unknown(6)),
        ] {
            assert_eq!(EndReason::from_mpv(raw), reason, "reason {raw}");
        }
    }

    /// Error codes from `enum mpv_error` in libmpv's client.h: -14 is
    /// MPV_ERROR_AO_INIT_FAILED, -13 MPV_ERROR_LOADING_FAILED.
    #[test]
    fn end_file_error_names_output_failure() {
        assert_eq!(
            EndReason::from_end_file(4, -14),
            EndReason::AudioOutputFailed
        );
        assert_eq!(EndReason::from_end_file(4, -13), EndReason::Error);
        assert_eq!(EndReason::from_end_file(0, 0), EndReason::Eof);
    }
}
