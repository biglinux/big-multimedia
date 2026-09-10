//! Audio filter-chain tests — HPF, compressor, gate, EQ, downmix, atempo chain.

use super::*;

fn nr_job(channels: u32) -> FfmpegJob {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.noise_reduction.enabled = true;
    let mut job = base_job(profile);
    job.input_audio_channels = channels;
    job
}

#[test]
fn nr_multichannel_layout_and_pan_per_channel_count() {
    // Each channel count selects a distinct channelsplit layout AND a distinct
    // pan downmix — pins every layout/pan match arm and the `channels > 1` gate.
    for (ch, layout, pan) in [
        (3, "3.0", "pan=3.0|FL=c0|FR=c1|FC=c2"),
        (4, "quad", "pan=quad|FL=c0|FR=c1|BL=c2|BR=c3"),
        (5, "5.0", "pan=5.0|FL=c0|FR=c1|FC=c2|BL=c3|BR=c4"),
        (6, "5.1", "pan=5.1|FL=c0|FR=c1|FC=c2|LFE=c3|BL=c4|BR=c5"),
        (
            7,
            "6.1",
            "pan=6.1|FL=c0|FR=c1|FC=c2|LFE=c3|BL=c4|BR=c5|BC=c6",
        ),
        (
            8,
            "7.1",
            "pan=7.1|FL=c0|FR=c1|FC=c2|LFE=c3|BL=c4|BR=c5|SL=c6|SR=c7",
        ),
    ] {
        let (af, fc) = nr_job(ch).build_audio_filters();
        assert!(fc, "ch{ch} must use the split graph");
        assert!(
            af.contains(&format!("channel_layout={layout}")),
            "ch{ch} layout: {af}"
        );
        assert!(af.contains(pan), "ch{ch} pan: {af}");
        assert!(
            af.contains(&format!("amerge=inputs={ch}")),
            "ch{ch} merge: {af}"
        );
    }
    // Two channels: layout falls through to "stereo", pan uses its arm 2.
    let (af2, fc2) = nr_job(2).build_audio_filters();
    assert!(fc2);
    assert!(af2.contains("channel_layout=stereo"), "{af2}");
    assert!(af2.contains("pan=stereo|FL=c0|FR=c1"), "{af2}");
    // One channel is below the `channels > 1` gate → no split graph.
    let (_, fc1) = nr_job(1).build_audio_filters();
    assert!(!fc1, "single channel must not use the split graph");
}

#[test]
fn audio_filters_none_handling_is_passthrough() {
    let mut profile = dummy_profile();
    profile.audio.handling = "none".into();
    let (af, fc) = base_job(profile).build_audio_filters();
    assert!(af.is_empty());
    assert!(!fc);
}

#[test]
fn copy_handling_overrides_enabled_effects() {
    // "copy" is a hard passthrough — even with an effect enabled the filter
    // chain stays empty (pins the `copy || none` disjunction; with no effects
    // the build is empty anyway, so an enabled effect is needed to observe it).
    let mut profile = dummy_profile();
    profile.audio.handling = "copy".into();
    profile.audio.effects.hpf_enabled = true;
    profile.audio.effects.hpf_frequency = 120;
    let (af, fc) = base_job(profile).build_audio_filters();
    assert!(af.is_empty(), "copy must not build effect filters: {af}");
    assert!(!fc);
}

#[test]
fn eq_disabled_ignores_present_bands() {
    // eq_enabled=false must skip the band loop even when bands are set
    // (pins the `eq_enabled && !bands.is_empty()` conjunction).
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.eq_enabled = false;
    profile.audio.effects.eq_bands = vec![6.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    assert!(
        !base_job(profile)
            .build_audio_filters()
            .0
            .contains("equalizer"),
        "disabled EQ must emit nothing"
    );
}

#[test]
fn hpf_disabled_at_zero_frequency() {
    // hpf_enabled but frequency == 0 must NOT add a highpass (strict `> 0`).
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.hpf_enabled = true;
    profile.audio.effects.hpf_frequency = 0;
    assert!(
        !base_job(profile)
            .build_audio_filters()
            .0
            .contains("highpass"),
        "zero-frequency hpf must be skipped"
    );
}

#[test]
fn eq_band_at_threshold_is_skipped() {
    // |gain| must EXCEED 0.1 to emit a band (strict `>`): a band of exactly
    // 0.1 produces no equalizer filter.
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.eq_enabled = true;
    profile.audio.effects.eq_bands = vec![0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    assert!(
        !base_job(profile)
            .build_audio_filters()
            .0
            .contains("equalizer"),
        "a 0.1 dB band is within the dead-band"
    );
}

#[test]
fn unity_volume_emits_no_volume_filter() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.volume = 1.0;
    assert!(
        !base_job(profile)
            .build_audio_filters()
            .0
            .contains("volume="),
        "unit gain is a no-op"
    );
}

#[test]
fn speed_atempo_boundaries_and_chaining() {
    let make = |speed: f64| {
        let mut profile = dummy_profile();
        profile.audio.handling = "encode".into();
        profile.audio.effects.speed = speed;
        base_job(profile).build_audio_filters().0
    };
    // Exactly 0.5 needs no split (strict `< 0.5`): a single atempo=0.5000.
    assert_eq!(make(0.5), "atempo=0.5000");
    // Exactly 100 needs no split (strict `> 100`): a single atempo=100.0000.
    assert_eq!(make(100.0), "atempo=100.0000");
    // 200 splits once: atempo=100.0 then the 2.0 remainder (pins `/= 100`).
    assert_eq!(make(200.0), "atempo=100.0,atempo=2.0000");
}

#[test]
fn test_audio_filters_copy_returns_empty() {
    let mut profile = dummy_profile();
    profile.audio.handling = "copy".into();
    assert!(base_job(profile).build_audio_filters().0.is_empty());
}

#[test]
fn test_audio_filters_hpf() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.hpf_enabled = true;
    profile.audio.effects.hpf_frequency = 120;
    assert_eq!(
        base_job(profile).build_audio_filters().0,
        "highpass=f=120:poles=2"
    );
}

#[test]
fn test_audio_filters_compressor() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.compressor_enabled = true;
    profile.audio.effects.compressor_intensity = 0.5;
    let (af, _fc) = base_job(profile).build_audio_filters();
    assert!(af.contains("acompressor="), "got: {af}");
    assert!(af.contains("threshold="), "got: {af}");
}

#[test]
fn test_audio_filters_normalize() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.normalize = true;
    assert_eq!(
        base_job(profile).build_audio_filters().0,
        "speechnorm=e=12.5:r=0.0001:l=1"
    );
}

#[test]
fn test_audio_filters_eq_bands() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.eq_enabled = true;
    profile.audio.effects.eq_bands = vec![0.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0];
    let (af, _fc) = base_job(profile).build_audio_filters();
    assert!(af.contains("equalizer=f=1000"), "got: {af}");
    assert!(af.contains("g=3.0"), "got: {af}");
}

#[test]
fn test_audio_filters_gate() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.gate_enabled = true;
    profile.audio.effects.compressor_intensity = 0.5;
    let (af, _fc) = base_job(profile).build_audio_filters();
    assert!(af.contains("agate="), "got: {af}");
}

#[test]
fn test_audio_filters_channel_downmix() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.channels = "stereo".into();
    assert_eq!(
        base_job(profile).build_audio_filters().0,
        "aformat=channel_layouts=stereo"
    );
}

#[test]
fn test_audio_filters_volume() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.volume = 1.25;
    let (af, _fc) = base_job(profile).build_audio_filters();
    assert!(af.contains("volume=1.2500"), "got: {af}");
}

#[test]
fn test_audio_filters_speed_chained() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.speed = 0.2;
    let (af, _fc) = base_job(profile).build_audio_filters();
    assert!(
        af.contains("atempo=0.5,atempo=0.5,atempo=0.8000"),
        "got: {af}"
    );
}
