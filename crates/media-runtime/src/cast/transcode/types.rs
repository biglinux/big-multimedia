use crate::cast::gpu::GpuType;

/// Audio/video filter configuration for transcoding.
#[derive(Debug, Clone)]
pub struct TranscodeFilters {
    /// Voice enhancement via GTCRN LADSPA plugin.
    pub voice_enhance: bool,
    /// Noise reduction strength (0.0–1.0).
    pub noise_strength: f32,
    /// Equalizer bands: (frequency_hz, gain_db). Empty = no EQ.
    pub eq_bands: Vec<(f32, f32)>,
    /// Video brightness (-1.0 to 1.0, 0.0 = neutral).
    pub brightness: f32,
    /// Video contrast (0.0 to 2.0, 1.0 = neutral).
    pub contrast: f32,
    /// Video saturation (0.0 to 3.0, 1.0 = neutral).
    pub saturation: f32,
    /// Sharpness via unsharp mask (0.0 to 2.0, 0.0 = off).
    pub sharpness: f32,
    /// Audio volume multiplier (1.0 = 100%, 0.5 = 50%, 1.5 = 150%). 1.0 = no change.
    pub volume: f32,
    /// Playback speed multiplier (1.0 = normal, 2.0 = 2x). 1.0 = no change.
    pub speed: f32,
    /// Audio track index within the file (0 = first audio). None = default.
    pub audio_track: Option<usize>,
    /// Subtitle track index within the file (0 = first subtitle). None = no subtitles.
    pub subtitle_track: Option<usize>,
    /// Whether the selected subtitle is bitmap-based (PGS, VOBSUB, DVB).
    /// Text subs use the `subtitles` filter; bitmap subs use `-filter_complex` overlay.
    pub subtitle_is_bitmap: bool,
    /// Horizontal flip.
    pub hflip: bool,
    /// Vertical flip.
    pub vflip: bool,
    /// Rotation in degrees (0, 90, 180, 270).
    pub rotation: i32,
    /// Force a specific GPU type (None = auto-detect).
    pub gpu_override: Option<GpuType>,
    /// Force software decode + GPU encode (Stage 2 / partial GPU mode).
    pub software_decode: bool,
    /// GPU device path (e.g. "/dev/dri/renderD128"). Empty = default.
    pub gpu_device_path: String,
}

impl Default for TranscodeFilters {
    fn default() -> Self {
        Self {
            voice_enhance: false,
            noise_strength: 0.75,
            eq_bands: Vec::new(),
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            sharpness: 0.0,
            volume: 1.0,
            speed: 1.0,
            audio_track: None,
            subtitle_track: None,
            subtitle_is_bitmap: false,
            hflip: false,
            vflip: false,
            rotation: 0,
            gpu_override: None,
            software_decode: false,
            gpu_device_path: String::new(),
        }
    }
}

impl TranscodeFilters {
    /// Whether any audio filter is active.
    #[must_use]
    pub fn has_audio_filters(&self) -> bool {
        self.voice_enhance || !self.eq_bands.is_empty() || (self.volume - 1.0).abs() > f32::EPSILON
    }

    /// Whether any video filter is active.
    #[must_use]
    pub fn has_video_filters(&self) -> bool {
        self.brightness.abs() > f32::EPSILON
            || (self.contrast - 1.0).abs() > f32::EPSILON
            || (self.saturation - 1.0).abs() > f32::EPSILON
            || self.sharpness > 0.0
            || self.subtitle_track.is_some()
            || self.hflip
            || self.vflip
            || self.rotation != 0
    }

    /// Whether any filter is active.
    #[must_use]
    pub fn has_any_filter(&self) -> bool {
        self.has_audio_filters()
            || self.has_video_filters()
            || self.audio_track.is_some()
            || (self.speed - 1.0).abs() > f32::EPSILON
    }
}
