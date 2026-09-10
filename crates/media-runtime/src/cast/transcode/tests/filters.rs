use super::*;

#[test]
fn default_filters_are_neutral() {
    let f = TranscodeFilters::default();
    assert!(!f.voice_enhance);
    assert!(f.eq_bands.is_empty());
    assert!((f.brightness).abs() < f32::EPSILON);
    assert!((f.contrast - 1.0).abs() < f32::EPSILON);
    assert!((f.saturation - 1.0).abs() < f32::EPSILON);
    assert!((f.volume - 1.0).abs() < f32::EPSILON);
    assert!((f.speed - 1.0).abs() < f32::EPSILON);
    assert_eq!(f.rotation, 0);
    assert!(!f.hflip);
    assert!(!f.vflip);
}

#[test]
fn default_has_no_filters() {
    let f = TranscodeFilters::default();
    assert!(!f.has_audio_filters());
    assert!(!f.has_video_filters());
    assert!(!f.has_any_filter());
}

#[test]
fn voice_enhance_triggers_audio_filter() {
    let mut f = TranscodeFilters::default();
    f.voice_enhance = true;
    assert!(f.has_audio_filters());
    assert!(f.has_any_filter());
}

#[test]
fn eq_bands_trigger_audio_filter() {
    let mut f = TranscodeFilters::default();
    f.eq_bands = vec![(1000.0, 3.0)];
    assert!(f.has_audio_filters());
}

#[test]
fn volume_change_triggers_audio_filter() {
    let mut f = TranscodeFilters::default();
    f.volume = 0.5;
    assert!(f.has_audio_filters());
}

#[test]
fn unity_volume_no_audio_filter() {
    let f = TranscodeFilters::default();
    assert!(!f.has_audio_filters());
}

#[test]
fn brightness_triggers_video_filter() {
    let mut f = TranscodeFilters::default();
    f.brightness = 0.1;
    assert!(f.has_video_filters());
}

#[test]
fn contrast_triggers_video_filter() {
    let mut f = TranscodeFilters::default();
    f.contrast = 1.5;
    assert!(f.has_video_filters());
}

#[test]
fn saturation_triggers_video_filter() {
    let mut f = TranscodeFilters::default();
    f.saturation = 0.0;
    assert!(f.has_video_filters());
}

#[test]
fn subtitle_triggers_video_filter() {
    let mut f = TranscodeFilters::default();
    f.subtitle_track = Some(0);
    assert!(f.has_video_filters());
}

#[test]
fn hflip_triggers_video_filter() {
    let mut f = TranscodeFilters::default();
    f.hflip = true;
    assert!(f.has_video_filters());
}

#[test]
fn vflip_triggers_video_filter() {
    let mut f = TranscodeFilters::default();
    f.vflip = true;
    assert!(f.has_video_filters());
}

#[test]
fn rotation_triggers_video_filter() {
    let mut f = TranscodeFilters::default();
    f.rotation = 90;
    assert!(f.has_video_filters());
}

#[test]
fn speed_triggers_any_filter_but_not_audio_or_video() {
    let mut f = TranscodeFilters::default();
    f.speed = 2.0;
    assert!(!f.has_audio_filters());
    assert!(!f.has_video_filters());
    assert!(f.has_any_filter());
}

#[test]
fn audio_track_triggers_any_filter() {
    let mut f = TranscodeFilters::default();
    f.audio_track = Some(1);
    assert!(f.has_any_filter());
}

#[test]
fn ladspa_available_does_not_panic() {
    let _ = big_audio_effects::noise_reduction::gtcrn_available();
}
