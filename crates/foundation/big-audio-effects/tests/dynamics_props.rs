//! Property tests for `GateDerived`, `CompressorDerived`, EQ preset round-trip.

use big_audio_effects::dynamics::{CompressorDerived, GateDerived};
use big_audio_effects::equalizer::{EQ_PRESET_IDS, EQ_PRESET_LABELS, eq_preset_index};
use proptest::prelude::*;

proptest! {
    // Miri interprets every generated case and turns these randomized property
    // tests into a deep-gate bottleneck. Ordinary cargo test still covers the
    // property space; deterministic tests below remain available to Miri.
    // GateDerived bounds: t ∈ [0,1] yields documented ranges.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn gate_bounds(t in 0.0_f64..=1.0_f64) {
        let g = GateDerived::from_unit_intensity(t);
        // threshold_db ∈ [-55, -30]
        prop_assert!(g.threshold_db >= -55.0 - 1e-9 && g.threshold_db <= -30.0 + 1e-9,
            "threshold_db out of range: {}", g.threshold_db);
        // range_db ∈ [-40, -18]
        prop_assert!(g.range_db >= -40.0 - 1e-9 && g.range_db <= -18.0 + 1e-9,
            "range_db out of range: {}", g.range_db);
        prop_assert!(g.attack_ms == 2.0);
        // hold_ms ∈ [100, 150]
        prop_assert!(g.hold_ms >= 100.0 - 1e-9 && g.hold_ms <= 150.0 + 1e-9);
        // release_ms ∈ [150, 250]
        prop_assert!(g.release_ms >= 150.0 - 1e-9 && g.release_ms <= 250.0 + 1e-9);
    }

    // Gate threshold monotonic non-decreasing on intensity.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn gate_threshold_monotonic(a in 0.0_f64..=1.0_f64, b in 0.0_f64..=1.0_f64) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let g_lo = GateDerived::from_unit_intensity(lo);
        let g_hi = GateDerived::from_unit_intensity(hi);
        prop_assert!(g_lo.threshold_db <= g_hi.threshold_db + 1e-9);
        // range_db non-increasing (more negative as intensity rises)
        prop_assert!(g_lo.range_db >= g_hi.range_db - 1e-9);
    }

    // CompressorDerived bounds.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn compressor_bounds(t in 0.0_f64..=1.0_f64) {
        let c = CompressorDerived::from_unit_intensity(t);
        // threshold_db ∈ [-30, -15]
        prop_assert!(c.threshold_db >= -30.0 - 1e-9 && c.threshold_db <= -15.0 + 1e-9);
        // ratio ∈ [2, 6]
        prop_assert!(c.ratio >= 2.0 - 1e-9 && c.ratio <= 6.0 + 1e-9);
        // makeup ∈ [2, 10]
        prop_assert!(c.makeup_gain_db >= 2.0 - 1e-9 && c.makeup_gain_db <= 10.0 + 1e-9);
        // knee ∈ [3, 8]
        prop_assert!(c.knee_db >= 3.0 - 1e-9 && c.knee_db <= 8.0 + 1e-9);
        prop_assert!(c.attack_ms == 10.0);
        prop_assert!(c.release_ms == 100.0);
    }

    // Compressor ratio monotonic non-decreasing; threshold monotonic non-increasing.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn compressor_monotonic(a in 0.0_f64..=1.0_f64, b in 0.0_f64..=1.0_f64) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let c_lo = CompressorDerived::from_unit_intensity(lo);
        let c_hi = CompressorDerived::from_unit_intensity(hi);
        prop_assert!(c_lo.ratio <= c_hi.ratio + 1e-9);
        prop_assert!(c_lo.threshold_db >= c_hi.threshold_db - 1e-9);
        prop_assert!(c_lo.makeup_gain_db <= c_hi.makeup_gain_db + 1e-9);
    }

    // Out-of-range clamping: extreme inputs collapse to endpoints.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn intensity_clamped(t in -1000.0_f64..1000.0_f64) {
        let g = GateDerived::from_unit_intensity(t);
        // threshold still in [-55, -30]
        prop_assert!(g.threshold_db >= -55.0 - 1e-9 && g.threshold_db <= -30.0 + 1e-9);
        let c = CompressorDerived::from_unit_intensity(t);
        prop_assert!(c.ratio >= 2.0 - 1e-9 && c.ratio <= 6.0 + 1e-9);
    }
}

// EQ preset id ↔ label round-trip — index from id matches label position.
#[test]
fn eq_preset_index_round_trip() {
    assert_eq!(EQ_PRESET_IDS.len(), EQ_PRESET_LABELS.len());
    for (idx, id) in EQ_PRESET_IDS.iter().enumerate() {
        assert_eq!(eq_preset_index(id), idx, "round-trip failed for {id}");
        // every label is non-empty
        assert!(!EQ_PRESET_LABELS[idx].is_empty(), "label {idx} empty");
    }
    // unknown id → 0 (flat)
    assert_eq!(eq_preset_index(""), 0);
    assert_eq!(eq_preset_index("nonexistent-preset-xyz"), 0);
}
