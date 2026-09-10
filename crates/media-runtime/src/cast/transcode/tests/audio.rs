use super::*;

#[test]
fn audio_filters_default_empty() {
    let s = make_session(TranscodeFilters::default());
    assert_eq!(s.build_audio_filters(), "");
}

#[test]
fn audio_filters_voice_enhance_adds_highpass_and_compressor() {
    let mut f = TranscodeFilters::default();
    f.voice_enhance = true;
    let s = make_session(f);
    let result = s.build_audio_filters();
    assert!(result.contains("highpass=f=80:poles=2"));
    assert!(result.contains("acompressor=threshold=-20dB:ratio=4:attack=5:release=250"));
}

#[test]
fn audio_filters_eq_bands_emitted() {
    let mut f = TranscodeFilters::default();
    f.eq_bands = vec![(1000.0, 3.0), (4000.0, -2.0)];
    let s = make_session(f);
    let result = s.build_audio_filters();
    assert!(result.contains("highpass=f=80:poles=2"));
    assert!(result.contains("equalizer=f=1000:width_type=o:width=1:g=3.0"));
    assert!(result.contains("equalizer=f=4000:width_type=o:width=1:g=-2.0"));
}

#[test]
fn audio_filters_eq_zero_gain_skipped() {
    let mut f = TranscodeFilters::default();
    f.eq_bands = vec![(1000.0, 0.0), (4000.0, 5.0)];
    let s = make_session(f);
    let result = s.build_audio_filters();
    assert!(!result.contains("f=1000"));
    assert!(result.contains("f=4000"));
}

#[test]
fn audio_filters_volume_half() {
    let mut f = TranscodeFilters::default();
    f.volume = 0.5;
    let s = make_session(f);
    let result = s.build_audio_filters();
    assert!(result.contains("volume=0.5000"));
}

#[test]
fn audio_filters_unity_volume_omitted() {
    let mut f = TranscodeFilters::default();
    f.voice_enhance = true;
    let s = make_session(f);
    let result = s.build_audio_filters();
    assert!(!result.contains("volume="));
}

#[test]
fn audio_filters_speed_2x() {
    let mut f = TranscodeFilters::default();
    f.speed = 2.0;
    f.volume = 0.5;
    let s = make_session(f);
    let result = s.build_audio_filters();
    assert!(result.contains("atempo=2.0000"));
}

#[test]
fn audio_filters_speed_normal_omitted() {
    let mut f = TranscodeFilters::default();
    f.voice_enhance = true;
    let s = make_session(f);
    let result = s.build_audio_filters();
    assert!(!result.contains("atempo"));
}

#[test]
fn audio_filters_chain_order() {
    let mut f = TranscodeFilters::default();
    f.eq_bands = vec![(1000.0, 3.0)];
    f.volume = 0.8;
    f.speed = 1.5;
    let s = make_session(f);
    let result = s.build_audio_filters();
    let parts: Vec<&str> = result.split(',').collect();
    let hp_pos = parts
        .iter()
        .position(|p| p.starts_with("highpass"))
        .unwrap();
    let eq_pos = parts
        .iter()
        .position(|p| p.starts_with("equalizer"))
        .unwrap();
    let vol_pos = parts.iter().position(|p| p.starts_with("volume")).unwrap();
    let at_pos = parts.iter().position(|p| p.starts_with("atempo")).unwrap();
    assert!(hp_pos < eq_pos, "highpass should precede eq");
    assert!(eq_pos < vol_pos, "eq should precede volume");
    assert!(vol_pos < at_pos, "volume should precede atempo");
}
