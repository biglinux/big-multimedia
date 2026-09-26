// SPDX-License-Identifier: MIT

//! Derived gate and compressor values shared by playback and conversion.
//!
//! The two structs in this module, [`crate::dynamics::GateDerived`] and
//! [`crate::dynamics::CompressorDerived`], map a single user-facing
//! intensity slider in `0.0..=1.0` to a calibrated set of low-level
//! parameters (thresholds, ratios, timings). Both the mpv-based players
//! and the FFmpeg-based converters derive their filter strings from
//! these structs so that the audible effect of moving the slider stays
//! identical across hosts.
//!
//! Two FFmpeg filter formatters,
//! [`crate::dynamics::ffmpeg_agate_filter`] and
//! [`crate::dynamics::ffmpeg_acompressor_filter`], assemble the actual
//! command-line filter string used by the conversion pipeline. The
//! helper [`crate::dynamics::percent_or_unit`] lets legacy callers that
//! still pass `0..=100` percentages share the same derivation curve.

/// Resolved noise-gate parameters derived from a single intensity value.
///
/// Construct with [`GateDerived::from_unit_intensity`] (or its
/// percent-tolerant sibling) to obtain the threshold, range and timing
/// values for one slider position. The fields mirror the parameters of
/// the FFmpeg `agate` filter so callers can hand them straight to the
/// filter formatter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateDerived {
    /// Open threshold in dBFS. Signals below this level are gated.
    pub threshold_db: f64,
    /// Attenuation applied to gated signal, in dB (negative value).
    pub range_db: f64,
    /// Attack time in milliseconds before the gate opens.
    pub attack_ms: f64,
    /// Hold time in milliseconds after the threshold is crossed downwards.
    pub hold_ms: f64,
    /// Release time in milliseconds for closing the gate.
    pub release_ms: f64,
}

impl GateDerived {
    /// Derive gate parameters from a unit-interval intensity.
    ///
    /// `intensity` is clamped to `0.0..=1.0`. The curve uses a square-root
    /// shaping so small slider moves near zero already produce an audible
    /// effect while the upper half stays smooth.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_audio_effects::dynamics::GateDerived;
    ///
    /// let gate = GateDerived::from_unit_intensity(0.0);
    /// assert_eq!(gate.threshold_db, -55.0);
    /// ```
    #[must_use]
    pub fn from_unit_intensity(intensity: f64) -> Self {
        let n = intensity.clamp(0.0, 1.0);
        let sqrt_n = n.sqrt();
        Self {
            threshold_db: -55.0 + sqrt_n * 25.0,
            range_db: -18.0 - sqrt_n * 22.0,
            attack_ms: 2.0,
            hold_ms: 150.0 - sqrt_n * 50.0,
            release_ms: 250.0 - sqrt_n * 100.0,
        }
    }

    /// Derive gate parameters accepting either `0..=1` or legacy `0..=100`.
    ///
    /// Calls [`percent_or_unit`] on the input first, so older settings
    /// files that store the slider as a percentage map onto the same
    /// curve as new unit-scale callers.
    #[must_use]
    pub fn from_percent_or_unit_intensity(intensity: f64) -> Self {
        Self::from_unit_intensity(percent_or_unit(intensity))
    }
}

/// Resolved compressor parameters derived from a single intensity value.
///
/// Mirrors the parameters of the FFmpeg `acompressor` filter so callers
/// can hand the struct straight to [`ffmpeg_acompressor_filter`]-style
/// formatters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressorDerived {
    /// Knee-center threshold in dBFS.
    pub threshold_db: f64,
    /// Compression ratio (input dB per 1 dB output above threshold).
    pub ratio: f64,
    /// Attack time in milliseconds.
    pub attack_ms: f64,
    /// Release time in milliseconds.
    pub release_ms: f64,
    /// Post-compression makeup gain in dB.
    pub makeup_gain_db: f64,
    /// Soft-knee width in dB centered on `threshold_db`.
    pub knee_db: f64,
    /// RMS-vs-peak detection bias, `0.0` for pure RMS, `1.0` for pure peak.
    pub rms_peak: f64,
}

impl CompressorDerived {
    /// Derive compressor parameters from a unit-interval intensity.
    ///
    /// `intensity` is clamped to `0.0..=1.0`. The curve scales threshold,
    /// ratio, makeup gain and knee linearly so the audible loudness gain
    /// from end to end is roughly even.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_audio_effects::dynamics::CompressorDerived;
    ///
    /// let comp = CompressorDerived::from_unit_intensity(1.0);
    /// assert_eq!(comp.ratio, 6.0);
    /// ```
    #[must_use]
    pub fn from_unit_intensity(intensity: f64) -> Self {
        let i = intensity.clamp(0.0, 1.0);
        Self {
            threshold_db: -15.0 - i * 15.0,
            ratio: 2.0 + i * 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            makeup_gain_db: 2.0 + i * 8.0,
            knee_db: 3.0 + i * 5.0,
            rms_peak: 0.0,
        }
    }
}

/// Normalize an intensity value to the unit interval.
///
/// Inputs above `1.0` are treated as percentages and divided by `100`.
/// The result is then clamped to `0.0..=1.0`. Lets a single API support
/// both modern unit-scale UIs and older percentage-based settings.
#[must_use]
pub fn percent_or_unit(intensity: f64) -> f64 {
    if intensity > 1.0 {
        intensity / 100.0
    } else {
        intensity
    }
    .clamp(0.0, 1.0)
}

/// Convert a decibel value to a linear amplitude multiplier.
///
/// Uses the natural-log form `exp(db / 20 * ln(10))` to match the
/// precision FFmpeg expects for filter arguments that take linear gain.
#[must_use]
pub fn db_to_linear(db: f64) -> f64 {
    (db / 20.0 * 10.0_f64.ln()).exp()
}

/// Build an FFmpeg `agate=...` filter string for the given intensity.
///
/// Accepts both unit (`0..=1`) and percent (`0..=100`) intensities via
/// [`GateDerived::from_percent_or_unit_intensity`].
///
/// # Examples
///
/// ```
/// use big_audio_effects::dynamics::ffmpeg_agate_filter;
///
/// let filter = ffmpeg_agate_filter(0.5);
/// assert!(filter.starts_with("agate="));
/// ```
#[must_use]
pub fn ffmpeg_agate_filter(intensity: f64) -> String {
    let gate = GateDerived::from_percent_or_unit_intensity(intensity);
    let threshold = db_to_linear(gate.threshold_db);
    let range = db_to_linear(gate.range_db);
    format!(
        "agate=threshold={threshold:.6}:range={range:.6}:attack={:.0}:release={:.0}:ratio=4:detection=rms",
        gate.attack_ms, gate.release_ms
    )
}

/// Build an FFmpeg `acompressor=...` filter string for the given intensity.
///
/// `intensity` is interpreted on the unit-interval scale and clamped by
/// [`CompressorDerived::from_unit_intensity`].
///
/// # Examples
///
/// ```
/// use big_audio_effects::dynamics::ffmpeg_acompressor_filter;
///
/// let filter = ffmpeg_acompressor_filter(0.25);
/// assert!(filter.starts_with("acompressor="));
/// ```
#[must_use]
pub fn ffmpeg_acompressor_filter(intensity: f64) -> String {
    let comp = CompressorDerived::from_unit_intensity(intensity);
    let threshold = db_to_linear(comp.threshold_db);
    let makeup = db_to_linear(comp.makeup_gain_db);
    let knee = db_to_linear(comp.knee_db);
    format!(
        "acompressor=threshold={threshold:.6}:ratio={:.6}:attack={:.0}:release={:.0}:makeup={makeup:.6}:knee={knee:.6}:detection=rms",
        comp.ratio, comp.attack_ms, comp.release_ms
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {actual} to be close to {expected}"
        );
    }

    #[test]
    fn gate_curve_matches_calibrated_endpoints() {
        let min = GateDerived::from_unit_intensity(0.0);
        assert_close(min.threshold_db, -55.0);
        assert_close(min.range_db, -18.0);

        let max = GateDerived::from_unit_intensity(1.0);
        assert_close(max.threshold_db, -30.0);
        assert_close(max.range_db, -40.0);
    }

    #[test]
    fn gate_curve_matches_calibrated_midpoint() {
        let gate = GateDerived::from_unit_intensity(0.25);

        assert_close(gate.threshold_db, -42.5);
        assert_close(gate.range_db, -29.0);
        assert_close(gate.attack_ms, 2.0);
        assert_close(gate.hold_ms, 125.0);
        assert_close(gate.release_ms, 200.0);
    }

    #[test]
    fn percent_or_unit_accepts_legacy_percent_values() {
        assert_close(percent_or_unit(50.0), 0.5);
        assert_close(percent_or_unit(1.0), 1.0);
        assert_close(percent_or_unit(0.5), 0.5);
    }

    #[test]
    fn compressor_curve_clamps() {
        let high = CompressorDerived::from_unit_intensity(2.0);
        assert_close(high.ratio, 6.0);

        let low = CompressorDerived::from_unit_intensity(-1.0);
        assert_close(low.ratio, 2.0);
    }

    #[test]
    fn compressor_curve_matches_calibrated_midpoint() {
        let compressor = CompressorDerived::from_unit_intensity(0.25);

        assert_close(compressor.threshold_db, -18.75);
        assert_close(compressor.ratio, 3.0);
        assert_close(compressor.attack_ms, 10.0);
        assert_close(compressor.release_ms, 100.0);
        assert_close(compressor.makeup_gain_db, 4.0);
        assert_close(compressor.knee_db, 4.25);
        assert_close(compressor.rms_peak, 0.0);
    }

    #[test]
    fn db_to_linear_matches_known_decibel_points() {
        assert_close(db_to_linear(20.0), 10.0);
        assert_close(db_to_linear(-20.0), 0.1);
    }

    #[test]
    fn ffmpeg_filters_keep_expected_filter_names() {
        assert!(ffmpeg_agate_filter(50.0).starts_with("agate="));
        assert!(ffmpeg_acompressor_filter(0.5).starts_with("acompressor="));
    }
}
