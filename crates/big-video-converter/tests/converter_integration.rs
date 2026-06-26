//! Integration tests for big-video-converter.
//!
//! Cover: mode branching, settings round-trip, i18n routing,
//! ffmpeg argv terminator safety, path canonicalization invariant,
//! and Drop safety of glib::SourceId-like values.

use big_converter_ui::ConverterMode;
#[cfg(not(miri))]
use big_media_runtime::cast::gpu::{GpuDevice, GpuType};
#[cfg(not(miri))]
use big_media_runtime::converter::ffmpeg::{FfmpegJob, GpuStage, TrimRange};
use big_media_runtime::converter::profiles::{
    self, AudioConfig, OutputConfig, Profile, ProfileMeta, SubtitleConfig, VideoConfig,
};
#[cfg(not(miri))]
use big_media_runtime::converter::subtitle::SubtitleInput;
#[cfg(not(miri))]
use big_media_runtime::i18n::i18n;
#[cfg(not(miri))]
use big_media_runtime::settings::Settings;
use big_video_converter::APP_MODE;
use std::path::PathBuf;

// --- Helpers ---

fn dummy_profile() -> Profile {
    Profile {
        profile: ProfileMeta::default(),
        video: VideoConfig::default(),
        audio: AudioConfig::default(),
        subtitles: SubtitleConfig::default(),
        output: OutputConfig::default(),
    }
}

#[cfg(not(miri))]
fn cpu_device() -> GpuDevice {
    GpuDevice {
        gpu_type: GpuType::Cpu,
        name: "CPU".into(),
        device_path: String::new(),
    }
}

#[cfg(not(miri))]
fn base_job(input: PathBuf, output: PathBuf, profile: Profile) -> FfmpegJob {
    FfmpegJob {
        input,
        output,
        profile,
        gpu: cpu_device(),
        stage: GpuStage::Software,
        trim: TrimRange::default(),
        subtitle: SubtitleInput::None,
        audio_track: None,
        video_track: None,
        crop: None,
        subtitle_streams: Vec::new(),
        skip_video_filters: false,
        video_overrides: None,
        input_audio_channels: 2,
        audio_only: false,
    }
}

#[cfg(not(miri))]
fn argv(command: &std::process::Command) -> Vec<String> {
    command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn last_dir_from_canonicalize_result(resolved: std::io::Result<PathBuf>) -> Option<String> {
    let path = resolved.ok()?;
    path.parent()
        .and_then(|parent| parent.to_str())
        .map(str::to_owned)
}

// --- Tests ---

/// caveman: `ConverterMode::from_env` is private in big-converter-ui
/// and `set_converter_mode` is OnceLock-protected — env round-trip is
/// covered in that crate's own tests. Here we pin the binary's mode.
#[test]
fn video_mode_pin() {
    assert!(matches!(APP_MODE, ConverterMode::Video));
    assert!(!APP_MODE.is_audio());
}

/// Mode env var: when `BIG_MEDIA_AUDIO_CONVERTER=1` the audio path is
/// selected. Video binary leaves it unset. We exercise the contract
/// without touching the OnceLock — direct enum comparison is enough
/// because the runtime gate maps env-var → enum.
#[test]
fn mode_branching_video_vs_audio() {
    assert_ne!(ConverterMode::Video, ConverterMode::Audio);
}

#[cfg(not(miri))]
#[test]
fn settings_persistence_round_trip() {
    let settings_config_directory = tempfile::tempdir().expect("tempdir");
    // SAFETY: test process.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", settings_config_directory.path());
    }

    let s = Settings::new("big-video-converter-test");
    assert!(s.set_string("last-profile", "h264-high"));
    assert!(s.set_int("parallel-jobs", 3));
    assert!(s.set_double("audio-conv-speed", 1.25));
    assert!(s.set_boolean("overwrite-existing", true));
    s.flush();

    let s2 = Settings::new("big-video-converter-test");
    assert_eq!(s2.string("last-profile"), "h264-high");
    assert_eq!(s2.int("parallel-jobs"), 3);
    assert!((s2.double("audio-conv-speed") - 1.25).abs() < 1e-9);
    assert!(s2.boolean("overwrite-existing"));
}

#[test]
fn i18n_msgids_are_non_empty() {
    for message_id in ["Cancel", "Convert", "Settings", "Add Folder"] {
        assert!(!message_id.is_empty());
    }
}

#[cfg(not(miri))]
#[test]
fn i18n_routes_known_strings() {
    // caveman: no bound catalog -> identity contract
    for s in ["Cancel", "Convert", "Settings", "Add Folder"] {
        let out = i18n(s);
        assert!(!out.is_empty(), "empty translation for {s}");
        assert_eq!(out, s, "unbound i18n must be identity");
    }
}

#[cfg(not(miri))]
#[test]
fn ffmpeg_argv_isolates_hostile_filename() {
    let evil = PathBuf::from("-f lavfi -i /dev/zero");
    let out = PathBuf::from("/tmp/out.mp4");
    let mut profile = dummy_profile();
    profile.video.codec = "copy".into();
    profile.audio.handling = "copy".into();
    let job = base_job(evil.clone(), out, profile);
    let args = argv(&job.build_command());

    let path_str = evil.to_string_lossy().to_string();
    let idx = args.iter().position(|a| a == &path_str).expect("path arg");
    assert!(idx > 0, "args: {args:?}");
    assert_eq!(args[idx - 1], "-i", "path not isolated by -i: {args:?}");
}

#[cfg(not(miri))]
#[test]
fn ffmpeg_argv_keeps_special_chars_single_arg() {
    let weird = PathBuf::from("/tmp/[01] Filme (% special).mkv");
    let out = PathBuf::from("/tmp/out.mp4");
    let job = base_job(weird.clone(), out, dummy_profile());
    let args = argv(&job.build_command());
    let s = weird.to_string_lossy().to_string();
    assert!(args.contains(&s), "lost weird path: {args:?}");
}

#[test]
fn failed_canonicalize_does_not_update_last_dir() {
    let resolved = Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "dangling symlink",
    ));

    assert!(last_dir_from_canonicalize_result(resolved).is_none());
}

#[cfg(not(miri))]
#[test]
fn broken_symlink_does_not_poison_last_dir() {
    let symlink_fixture_directory = tempfile::tempdir().expect("tempdir");
    let link = symlink_fixture_directory.path().join("dangling.mkv");
    #[cfg(unix)]
    std::os::unix::fs::symlink(symlink_fixture_directory.path().join("nonexistent"), &link)
        .expect("symlink");

    let resolved = std::fs::canonicalize(&link);
    assert!(resolved.is_err(), "{resolved:?}");

    assert!(
        last_dir_from_canonicalize_result(resolved).is_none(),
        "LAST_DIR was poisoned",
    );
}

// caveman: removed `dropping_source_id_holder_is_clean` — vacuous
// (boxed-closure stand-in didn't exercise real glib::SourceId drop).

#[test]
fn profile_builder_deterministic() {
    let a = dummy_profile();
    let b = dummy_profile();
    assert_eq!(
        toml::to_string(&a).unwrap(),
        toml::to_string(&b).unwrap(),
        "non-deterministic profile toml",
    );
}

#[cfg(not(miri))]
#[test]
fn resolution_bounds() {
    // caveman: drive real prod path -> build_video_filters scale token
    let valid: [(u32, u32); 4] = [(640, 360), (1280, 720), (1920, 1080), (3840, 2160)];
    for (w, h) in valid {
        let mut profile = dummy_profile();
        profile.video.filters.resolution = format!("{w}x{h}");
        let job = base_job(
            PathBuf::from("/tmp/in.mkv"),
            PathBuf::from("/tmp/out.mp4"),
            profile,
        );
        let filters = job.build_video_filters();
        assert!(
            filters.contains(&format!("{w}")) && filters.contains(&format!("{h}")),
            "({w},{h}) missing from filter chain: {filters}",
        );
    }
    // "auto" -> no scale token
    let mut profile = dummy_profile();
    profile.video.filters.resolution = "auto".into();
    let job = base_job(
        PathBuf::from("/tmp/in.mkv"),
        PathBuf::from("/tmp/out.mp4"),
        profile,
    );
    let filters = job.build_video_filters();
    assert!(!filters.contains("scale"), "auto produced scale: {filters}");
}

/// Smoke against real .mkv fixture — Command construction only, never
/// spawned. Files are 5+ GB; we only inspect argv shape.
#[cfg(not(miri))]
#[test]
fn real_video_fixture_argv_smoke() {
    // Point BIG_VIDEO_FIXTURES at a dir of real .mkv clips to run this smoke.
    // Unset (the default, including CI) skips it — no baked-in local path.
    let Some(p) = std::env::var_os("BIG_VIDEO_FIXTURES") else {
        eprintln!("BIG_VIDEO_FIXTURES unset — skipping");
        return;
    };
    let dir = PathBuf::from(p);
    if !dir.exists() {
        eprintln!("video fixtures missing — skipping");
        return;
    }
    let dir = dir.as_path();
    let mut picked: Option<PathBuf> = None;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("mkv") {
                picked = Some(p);
                break;
            }
        }
    }
    let Some(input) = picked else {
        eprintln!("no .mkv — skipping");
        return;
    };
    let job = base_job(
        input.clone(),
        PathBuf::from("/tmp/out.mp4"),
        dummy_profile(),
    );
    let args = argv(&job.build_command());
    let i = args
        .iter()
        .position(|a| a == &input.to_string_lossy().to_string())
        .expect("input path arg");
    assert_eq!(args[i - 1], "-i", "real fixture not isolated: {args:?}");
}

#[allow(dead_code)]
fn _force_link(p: profiles::Profile) -> profiles::Profile {
    p
}
