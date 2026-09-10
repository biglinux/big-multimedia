//! Profile tests: TOML roundtrip, container/CRF resolution, output naming
//! template, audio output extension, partial-TOML defaults, save/load.

use std::path::PathBuf;

use super::output::is_leap;
use super::*;

fn default_profile() -> Profile {
    Profile {
        profile: ProfileMeta::default(),
        video: VideoConfig::default(),
        audio: AudioConfig::default(),
        subtitles: SubtitleConfig::default(),
        output: OutputConfig::default(),
    }
}

#[test]
fn test_default_profile_roundtrip() {
    let profile = default_profile();
    let toml_str = toml::to_string_pretty(&profile).unwrap();
    let parsed: Profile = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.video.codec, "h264");
    assert_eq!(parsed.audio.handling, "copy");
}

#[test]
fn test_resolve_container_auto() {
    let mut profile = default_profile();
    profile.video.codec = "vp9".into();
    assert_eq!(resolve_container(&profile), "webm");
}

#[test]
fn test_quality_to_crf() {
    assert_eq!(quality_to_crf("h264", "good"), 23);
    assert_eq!(quality_to_crf("h265", "veryhigh"), 18);
    assert_eq!(quality_to_crf("av1", "low"), 42);
}

#[test]
fn test_resolve_container_all_codecs() {
    let codecs_expected = [
        ("h264", "mp4"),
        ("h265", "mp4"),
        ("av1", "mkv"),
        ("vp9", "webm"),
        ("prores", "mov"),
        ("copy", "mkv"),
        ("unknown_codec", "mkv"),
    ];
    for (codec, expected) in codecs_expected {
        let mut profile = default_profile();
        profile.video.codec = codec.into();
        assert_eq!(resolve_container(&profile), expected, "codec={codec}");
    }
}

#[test]
fn test_resolve_container_explicit_override() {
    let mut profile = default_profile();
    profile.video.codec = "h264".into();
    profile.output.container = "mkv".into();
    assert_eq!(resolve_container(&profile), "mkv");
}

#[test]
fn test_resolve_container_video_container_override() {
    let mut profile = default_profile();
    profile.video.codec = "h264".into();
    profile.output.container = "auto".into();
    profile.video.container = "ts".into();
    assert_eq!(resolve_container(&profile), "ts");
}

#[test]
fn test_resolve_output_name_default_template() {
    let profile = default_profile();
    let name = resolve_output_name(&profile, &PathBuf::from("/home/user/video.avi"));
    assert_eq!(name, "video.mp4");
}

#[test]
fn test_resolve_output_name_custom_template() {
    let mut profile = default_profile();
    profile.output.naming = "{name}_{codec}_{quality}.{ext}".into();
    let name = resolve_output_name(&profile, &PathBuf::from("/tmp/clip.mov"));
    assert_eq!(name, "clip_h264_good.mp4");
}

#[test]
fn test_resolve_output_name_date_token() {
    let mut profile = default_profile();
    profile.output.naming = "{name}_{date}.{ext}".into();
    let name = resolve_output_name(&profile, &PathBuf::from("/tmp/clip.mov"));
    assert!(name.contains('-'), "should contain date, got: {name}");
    assert!(name.starts_with("clip_"), "got: {name}");
    assert!(
        PathBuf::from(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4")),
        "got: {name}"
    );
}

#[test]
fn test_resolve_audio_output_name_encoded_extension() {
    let mut profile = default_profile();
    profile.audio.handling = "encode".into();
    profile.audio.codec = "mp3".into();
    let name = resolve_audio_output_name(&profile, &PathBuf::from("/tmp/voice.mkv"));
    assert!(
        PathBuf::from(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
    );
}

#[test]
fn test_resolve_audio_output_name_copy_keeps_source_extension() {
    let mut profile = default_profile();
    profile.audio.handling = "copy".into();
    profile.audio.codec = "flac".into();
    let name = resolve_audio_output_name(&profile, &PathBuf::from("/tmp/track.flac"));
    assert!(
        PathBuf::from(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("flac"))
    );
}

#[test]
fn test_resolve_audio_output_name_prefers_explicit_container() {
    let mut profile = default_profile();
    profile.audio.handling = "encode".into();
    profile.audio.codec = "aac".into();
    profile.output.container = "m4a".into();
    let name = resolve_audio_output_name(&profile, &PathBuf::from("/tmp/voice.mkv"));
    assert!(
        PathBuf::from(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("m4a"))
    );
}

#[test]
fn test_quality_to_crf_all_h264() {
    let expected = [
        ("veryhigh", 16),
        ("high", 19),
        ("good", 23),
        ("medium", 28),
        ("low", 33),
        ("verylow", 38),
        ("superlow", 45),
        ("unknown", 23),
    ];
    for (quality, crf) in expected {
        assert_eq!(
            quality_to_crf("h264", quality),
            crf,
            "h264 quality={quality}"
        );
    }
}

#[test]
fn test_quality_to_crf_all_h265() {
    let expected = [
        ("veryhigh", 18),
        ("high", 22),
        ("good", 26),
        ("medium", 30),
        ("low", 34),
        ("verylow", 40),
        ("superlow", 48),
        ("unknown", 26),
    ];
    for (quality, crf) in expected {
        assert_eq!(
            quality_to_crf("h265", quality),
            crf,
            "h265 quality={quality}"
        );
    }
}

#[test]
fn test_quality_to_crf_all_av1() {
    let expected = [
        ("veryhigh", 20),
        ("high", 25),
        ("good", 30),
        ("medium", 36),
        ("low", 42),
        ("verylow", 50),
        ("superlow", 58),
        ("unknown", 30),
    ];
    for (quality, crf) in expected {
        assert_eq!(quality_to_crf("av1", quality), crf, "av1 quality={quality}");
    }
}

#[test]
fn test_quality_to_crf_all_vp9() {
    let expected = [
        ("veryhigh", 18),
        ("high", 24),
        ("good", 30),
        ("medium", 36),
        ("low", 42),
        ("verylow", 50),
        ("superlow", 58),
        ("unknown", 30),
    ];
    for (quality, crf) in expected {
        assert_eq!(quality_to_crf("vp9", quality), crf, "vp9 quality={quality}");
    }
}

#[test]
fn test_quality_to_crf_unknown_codec() {
    assert_eq!(quality_to_crf("xyz", "good"), 23);
}

#[test]
fn test_preset_to_ffmpeg_av1() {
    assert_eq!(preset_to_ffmpeg("av1", "veryfast"), "8");
    assert_eq!(preset_to_ffmpeg("av1", "fast"), "6");
    assert_eq!(preset_to_ffmpeg("av1", "balanced"), "4");
    assert_eq!(preset_to_ffmpeg("av1", "slow"), "2");
    assert_eq!(preset_to_ffmpeg("av1", "veryslow"), "1");
    assert_eq!(preset_to_ffmpeg("av1", "unknown"), "4");
}

#[test]
fn test_preset_to_ffmpeg_generic() {
    assert_eq!(preset_to_ffmpeg("h264", "veryfast"), "veryfast");
    assert_eq!(preset_to_ffmpeg("h264", "fast"), "fast");
    assert_eq!(preset_to_ffmpeg("h264", "slightlyfast"), "faster");
    assert_eq!(preset_to_ffmpeg("h264", "balanced"), "medium");
    assert_eq!(preset_to_ffmpeg("h264", "slow"), "slow");
    assert_eq!(preset_to_ffmpeg("h264", "veryslow"), "veryslow");
    assert_eq!(preset_to_ffmpeg("h264", "unknown"), "medium");
}

#[test]
fn test_partial_toml_uses_defaults() {
    let toml_str = r#"
[video]
codec = "h265"
"#;
    let parsed: Profile = toml::from_str(toml_str).unwrap();
    assert_eq!(parsed.video.codec, "h265");
    assert_eq!(parsed.video.quality, "good");
    assert_eq!(parsed.audio.handling, "copy");
    assert_eq!(parsed.subtitles.handling, "embedded");
}

#[test]
fn test_empty_toml_uses_all_defaults() {
    let parsed: Profile = toml::from_str("").unwrap();
    assert_eq!(parsed.video.codec, "h264");
    assert_eq!(parsed.video.quality, "good");
    assert_eq!(parsed.video.preset, "balanced");
    assert_eq!(parsed.audio.handling, "copy");
    assert_eq!(parsed.audio.codec, "aac");
}

#[test]
fn test_invalid_toml_returns_error() {
    let result: Result<Profile, _> = toml::from_str("this is not valid [toml");
    assert!(result.is_err());
}

#[test]
fn test_profile_with_filters_roundtrip() {
    let mut profile = default_profile();
    profile.video.filters.brightness = 0.5;
    profile.video.filters.contrast = 1.25;
    profile.video.filters.saturation = 1.5;
    profile.video.filters.rotation = 90;
    profile.video.filters.flip_h = true;
    profile.video.filters.crop = Some(CropRect {
        w: 640,
        h: 480,
        x: 10,
        y: 20,
    });
    let toml_str = toml::to_string_pretty(&profile).unwrap();
    let parsed: Profile = toml::from_str(&toml_str).unwrap();
    assert!((parsed.video.filters.brightness - 0.5).abs() < f64::EPSILON);
    assert!((parsed.video.filters.contrast - 1.25).abs() < f64::EPSILON);
    assert_eq!(parsed.video.filters.rotation, 90);
    assert!(parsed.video.filters.flip_h);
    let crop = parsed.video.filters.crop.unwrap();
    assert_eq!(crop.w, 640);
}

#[test]
fn test_profile_audio_effects_roundtrip() {
    let mut profile = default_profile();
    profile.audio.handling = "encode".into();
    profile.audio.effects.compressor_enabled = true;
    profile.audio.effects.compressor_intensity = 0.7;
    profile.audio.effects.eq_enabled = true;
    profile.audio.effects.eq_bands = vec![1.0, -2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let toml_str = toml::to_string_pretty(&profile).unwrap();
    let parsed: Profile = toml::from_str(&toml_str).unwrap();
    assert!(parsed.audio.effects.compressor_enabled);
    assert!((parsed.audio.effects.compressor_intensity - 0.7).abs() < f64::EPSILON);
    assert_eq!(parsed.audio.effects.eq_bands.len(), 10);
}

#[test]
fn test_profile_noise_reduction_roundtrip() {
    let mut profile = default_profile();
    profile.audio.noise_reduction.enabled = true;
    profile.audio.noise_reduction.strength = 0.8;
    profile.audio.noise_reduction.model = 1;
    let toml_str = toml::to_string_pretty(&profile).unwrap();
    let parsed: Profile = toml::from_str(&toml_str).unwrap();
    assert!(parsed.audio.noise_reduction.enabled);
    assert!((parsed.audio.noise_reduction.strength - 0.8).abs() < f64::EPSILON);
}

#[test]
fn test_profile_meta_defaults() {
    let meta = ProfileMeta::default();
    assert_eq!(meta.name, "Custom");
    assert_eq!(meta.category, "general");
    assert!(meta.description.is_empty());
}

#[test]
fn test_save_and_load_profile() {
    let temp_dir = tempfile::tempdir().unwrap();
    // SAFETY: scoped unit test; restore before returning.
    let previous_config = std::env::var_os("XDG_CONFIG_HOME");
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
    }

    let profile = default_profile();
    let path = save_user_profile("test_roundtrip.toml", &profile).unwrap();
    let loaded = load_profile(&path).unwrap();
    assert_eq!(loaded.video.codec, "h264");
    assert_eq!(loaded.audio.codec, "aac");

    unsafe {
        if let Some(value) = previous_config {
            std::env::set_var("XDG_CONFIG_HOME", value);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }
}

#[test]
fn test_load_profile_missing_file() {
    let result = load_profile(std::path::Path::new("/nonexistent/path/profile.toml"));
    assert!(result.is_err());
}

#[test]
fn test_is_leap() {
    assert!(is_leap(2000));
    assert!(is_leap(2024));
    assert!(!is_leap(1900));
    assert!(!is_leap(2023));
}
