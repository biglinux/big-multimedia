// SPDX-License-Identifier: MIT

//! Canonical 10-band equalizer frequencies and presets.
//!
//! This module fixes the public contract of the equalizer UI so that
//! presets saved by one BigLinux app continue to load identically in
//! another. It exposes:
//!
//! - The number of bands, their center frequencies (in three numeric
//!   widths) and the human-readable labels rendered under each slider.
//! - The catalog of generic listening presets
//!   ([`crate::equalizer::EQ_PRESET_IDS`],
//!   [`crate::equalizer::EQ_PRESET_LABELS`],
//!   [`crate::equalizer::EQ_PRESET_BANDS`]).
//! - The voice-tuning preset catalog used by the audio capture tools
//!   ([`crate::equalizer::VOICE_EQ_PRESET_IDS`],
//!   [`crate::equalizer::VOICE_EQ_PRESET_BANDS`]).
//!
//! Mutating any of these constants is a breaking change to user data.

/// Number of bands in the canonical equalizer.
pub const EQ_BAND_COUNT: usize = 10;

/// Canonical 10-band EQ center frequencies in Hz.
pub const EQ_BAND_FREQUENCIES_HZ: [u32; EQ_BAND_COUNT] =
    [31, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/// Canonical 10-band EQ center frequencies as `f64`.
///
/// Provided so DSP code that already works in double precision does not
/// need to convert each time.
pub const EQ_BAND_FREQUENCIES_F64: [f64; EQ_BAND_COUNT] = [
    31.0, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Canonical 10-band EQ center frequencies as `f32`.
///
/// Convenience copy for LADSPA control ports and other `f32`-typed APIs.
pub const EQ_BAND_FREQUENCIES_F32: [f32; EQ_BAND_COUNT] = [
    31.0, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Short labels rendered under each equalizer slider in the UI.
pub const EQ_BAND_LABELS: [&str; EQ_BAND_COUNT] = [
    "31", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k",
];

/// Stable identifiers for the generic listening presets.
///
/// Stored verbatim in user settings; the order matches
/// [`EQ_PRESET_LABELS`] and [`EQ_PRESET_BANDS`].
pub const EQ_PRESET_IDS: [&str; 6] = [
    "flat",
    "voice_clarity",
    "bass_boost",
    "treble_boost",
    "de_ess",
    "warmth",
];

/// Localizable display labels matching [`EQ_PRESET_IDS`].
pub const EQ_PRESET_LABELS: [&str; 6] = [
    "Flat",
    "Voice Clarity",
    "Bass Boost",
    "Treble Boost",
    "De-Ess",
    "Warmth",
];

/// Per-band gain in dB for each entry of [`EQ_PRESET_IDS`].
pub const EQ_PRESET_BANDS: [[f64; EQ_BAND_COUNT]; 6] = [
    [0.0; EQ_BAND_COUNT],
    [-2.0, -1.0, 0.0, 2.0, 4.0, 4.0, 3.0, 2.0, 0.0, -1.0],
    [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 5.0, 6.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -3.0, -4.0, -2.0],
    [3.0, 3.0, 2.0, 1.0, 0.0, 0.0, -1.0, -1.0, -2.0, -2.0],
];

/// Stable identifiers for the voice-tuning preset catalog.
///
/// Used by the noise-reduction/voice tooling. The `"custom"` entry
/// signals "use the user's stored per-band values" and intentionally
/// keeps the band array zeroed in [`VOICE_EQ_PRESET_BANDS`].
pub const VOICE_EQ_PRESET_IDS: [&str; 10] = [
    "default_voice",
    "flat",
    "voice_boost",
    "podcast",
    "warm",
    "bright",
    "de_esser",
    "bass_cut",
    "presence",
    "custom",
];

/// Per-band gain in dB for each entry of [`VOICE_EQ_PRESET_IDS`].
pub const VOICE_EQ_PRESET_BANDS: [[f32; EQ_BAND_COUNT]; 10] = [
    [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0, 0.0],
    [0.0; EQ_BAND_COUNT],
    [-10.0, -5.0, 0.0, 5.0, 15.0, 20.0, 15.0, 10.0, 5.0, 0.0],
    [5.0, 5.0, 10.0, 5.0, 0.0, 5.0, 10.0, 5.0, 0.0, -5.0],
    [10.0, 15.0, 10.0, 5.0, 0.0, -5.0, -10.0, -15.0, -15.0, -20.0],
    [-10.0, -5.0, 0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 20.0, 15.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, -5.0, -15.0, -25.0, -20.0, -10.0],
    [-40.0, -35.0, -25.0, -15.0, -5.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, -3.0, -2.0, 2.0, 8.0, 10.0, 5.0, 0.0],
    [0.0; EQ_BAND_COUNT],
];

/// FFmpeg `equalizer` filter bandwidth in octaves.
///
/// Matches the slope users hear in the mpv-based players, so converter
/// output renders the same EQ curve as live playback.
pub const CONVERTER_EQ_BANDWIDTH_OCTAVES: f64 = 1.5;

/// Resolve a preset identifier to its index in [`EQ_PRESET_IDS`].
///
/// Unknown identifiers fall back to index `0` (the flat preset) so
/// corrupted settings never throw the UI into an invalid state.
///
/// # Examples
///
/// ```
/// use big_audio_effects::equalizer::eq_preset_index;
///
/// assert_eq!(eq_preset_index("warmth"), 5);
/// assert_eq!(eq_preset_index("does-not-exist"), 0);
/// ```
#[must_use]
pub fn eq_preset_index(id: &str) -> usize {
    EQ_PRESET_IDS
        .iter()
        .position(|preset| *preset == id)
        .unwrap_or(0)
}

/// Look up the voice-EQ band array for a given preset identifier.
///
/// Returns `None` when `id` is not present in [`VOICE_EQ_PRESET_IDS`].
#[must_use]
pub fn voice_eq_preset_bands(id: &str) -> Option<&'static [f32; EQ_BAND_COUNT]> {
    VOICE_EQ_PRESET_IDS
        .iter()
        .position(|preset| *preset == id)
        .map(|idx| &VOICE_EQ_PRESET_BANDS[idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_contract_lengths_match() {
        assert_eq!(EQ_BAND_FREQUENCIES_HZ.len(), EQ_BAND_COUNT);
        assert_eq!(EQ_BAND_FREQUENCIES_F64.len(), EQ_BAND_COUNT);
        assert_eq!(EQ_BAND_FREQUENCIES_F32.len(), EQ_BAND_COUNT);
        assert_eq!(EQ_BAND_LABELS.len(), EQ_BAND_COUNT);
        assert_eq!(EQ_PRESET_IDS.len(), EQ_PRESET_LABELS.len());
        assert_eq!(EQ_PRESET_IDS.len(), EQ_PRESET_BANDS.len());
        assert_eq!(VOICE_EQ_PRESET_IDS.len(), VOICE_EQ_PRESET_BANDS.len());
    }

    #[test]
    fn eq_preset_index_defaults_to_flat() {
        assert_eq!(eq_preset_index("unknown"), 0);
        assert_eq!(eq_preset_index("warmth"), 5);
    }

    #[test]
    fn eq_preset_bands_match_public_contract() {
        assert_eq!(EQ_PRESET_BANDS[0], [0.0; EQ_BAND_COUNT]);
        assert_eq!(
            EQ_PRESET_BANDS[1],
            [-2.0, -1.0, 0.0, 2.0, 4.0, 4.0, 3.0, 2.0, 0.0, -1.0]
        );
        assert_eq!(
            EQ_PRESET_BANDS[2],
            [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            EQ_PRESET_BANDS[3],
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 5.0, 6.0]
        );
        assert_eq!(
            EQ_PRESET_BANDS[4],
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -3.0, -4.0, -2.0]
        );
        assert_eq!(
            EQ_PRESET_BANDS[5],
            [3.0, 3.0, 2.0, 1.0, 0.0, 0.0, -1.0, -1.0, -2.0, -2.0]
        );
    }

    #[test]
    fn voice_eq_preset_bands_match_public_contract() {
        assert_eq!(
            VOICE_EQ_PRESET_BANDS[0],
            [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0, 1.0, 0.0]
        );
        assert_eq!(VOICE_EQ_PRESET_BANDS[1], [0.0; EQ_BAND_COUNT]);
        assert_eq!(
            VOICE_EQ_PRESET_BANDS[2],
            [-10.0, -5.0, 0.0, 5.0, 15.0, 20.0, 15.0, 10.0, 5.0, 0.0]
        );
        assert_eq!(
            VOICE_EQ_PRESET_BANDS[3],
            [5.0, 5.0, 10.0, 5.0, 0.0, 5.0, 10.0, 5.0, 0.0, -5.0]
        );
        assert_eq!(
            VOICE_EQ_PRESET_BANDS[4],
            [10.0, 15.0, 10.0, 5.0, 0.0, -5.0, -10.0, -15.0, -15.0, -20.0]
        );
        assert_eq!(
            VOICE_EQ_PRESET_BANDS[5],
            [-10.0, -5.0, 0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 20.0, 15.0]
        );
        assert_eq!(
            VOICE_EQ_PRESET_BANDS[6],
            [0.0, 0.0, 0.0, 0.0, 0.0, -5.0, -15.0, -25.0, -20.0, -10.0]
        );
        assert_eq!(
            VOICE_EQ_PRESET_BANDS[7],
            [-40.0, -35.0, -25.0, -15.0, -5.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            VOICE_EQ_PRESET_BANDS[8],
            [0.0, 0.0, 0.0, -3.0, -2.0, 2.0, 8.0, 10.0, 5.0, 0.0]
        );
        assert_eq!(VOICE_EQ_PRESET_BANDS[9], [0.0; EQ_BAND_COUNT]);
    }

    #[test]
    fn voice_eq_preset_lookup_requires_exact_id() {
        assert_eq!(
            voice_eq_preset_bands("presence"),
            Some(&VOICE_EQ_PRESET_BANDS[8])
        );
        assert!(voice_eq_preset_bands("unknown").is_none());
    }

    #[test]
    fn presence_preset_keeps_peak_on_clarity_band() {
        let bands = voice_eq_preset_bands("presence").unwrap();
        let peak_idx = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(idx, _)| idx)
            .unwrap();
        assert!(peak_idx == 6 || peak_idx == 7);
        assert!(bands[2].abs() < f32::EPSILON);
        assert!(bands[9] <= 0.0);
    }
}
