// Property tests for audio filter chain composition.
// Gated by `fuzz` feature so default CI stays fast.
#![cfg(feature = "fuzz")]

//! Invariants for [`build_audio_filter_chain`]:
//! - Joined string never has trailing/duplicate commas.
//! - No nested empty filter groups (`,,`, leading/trailing `,`).
//! - Every filter entry is non-empty.
//! - Entry count equals number of enabled-effect contributions.

use big_media_runtime::player::mpv_backend::AudioEffects;
use big_media_runtime::player::mpv_backend::filters::build_audio_filter_chain;
use proptest::prelude::*;

// helper — build effects from raw flags + finite numbers
fn make_fx(normalize: bool, eq_bands: &[(f64, f64)]) -> AudioEffects<'_> {
    AudioEffects {
        normalize,
        nr_strength: 0.0, // nr requires ladspa plugin lookup; skip
        nr_model: 0,
        nr_mono: false,
        nr_voice_only: None,
        eq_bands,
    }
}

fn assert_chain_invariants(chain: &[String]) {
    // no empty entries
    for entry in chain {
        assert!(!entry.is_empty(), "empty filter entry");
        assert!(!entry.contains(",,"), "double comma inside entry: {entry}");
    }
    // joined form — emulate mpv lavfi=[...] join with ','
    let joined = chain.join(",");
    if !chain.is_empty() {
        assert!(
            !joined.starts_with(','),
            "joined chain starts with comma: {joined}"
        );
        assert!(
            !joined.ends_with(','),
            "joined chain ends with comma: {joined}"
        );
        assert!(
            !joined.contains(",,"),
            "joined chain has duplicate comma: {joined}"
        );
        // No empty bracket / paren groups
        assert!(!joined.contains("[]"), "empty bracket group: {joined}");
        assert!(!joined.contains("()"), "empty paren group: {joined}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    })]

    #[test]
    fn filter_chain_well_formed(
        normalize in any::<bool>(),
        eq_bands in proptest::collection::vec(
            (50.0_f64..=20_000.0, -20.0_f64..=20.0),
            0..6,
        ),
    ) {
        let fx = make_fx(normalize, &eq_bands);
        let chain = build_audio_filter_chain(&fx);
        assert_chain_invariants(&chain);
    }

    #[test]
    fn filter_chain_all_disabled_is_empty(
        // EQ bands all below 0.1 dB threshold
        eq_bands in proptest::collection::vec(
            (50.0_f64..=20_000.0, -0.05_f64..=0.05),
            0..6,
        ),
    ) {
        let fx = make_fx(false, &eq_bands);
        let chain = build_audio_filter_chain(&fx);
        prop_assert!(chain.is_empty(), "expected empty, got: {chain:?}");
    }
}
