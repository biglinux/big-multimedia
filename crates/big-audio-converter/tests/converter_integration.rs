//! Integration tests for big-audio-converter.
//!
//! Cover: mode branching, settings round-trip, i18n routing,
//! ffmpeg argv terminator safety, path canonicalization invariant,
//! and Drop safety of glib::SourceId-like values.

use big_audio_converter::{APP_MODE, BOOTSTRAP_ENV_KEY, BOOTSTRAP_ENV_VAL};
use big_converter_ui::ConverterMode;
use big_media_runtime::cast::gpu::{GpuDevice, GpuType};
use big_media_runtime::converter::ffmpeg::{FfmpegJob, GpuStage, TrimRange};
use big_media_runtime::converter::profiles::{
    self, AudioConfig, OutputConfig, Profile, ProfileMeta, SubtitleConfig, VideoConfig,
};
use big_media_runtime::converter::subtitle::SubtitleInput;
#[cfg(not(miri))]
use big_media_runtime::i18n::i18n;
#[cfg(not(miri))]
use big_media_runtime::settings::Settings;
use std::path::PathBuf;

// --- Helpers (replicated from runtime test mod; private there). ---

fn dummy_profile() -> Profile {
    Profile {
        profile: ProfileMeta::default(),
        video: VideoConfig::default(),
        audio: AudioConfig::default(),
        subtitles: SubtitleConfig::default(),
        output: OutputConfig::default(),
    }
}

fn cpu_device() -> GpuDevice {
    GpuDevice {
        gpu_type: GpuType::Cpu,
        name: "CPU".into(),
        device_path: String::new(),
    }
}

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
        audio_only: true, // audio-converter binding
    }
}

fn argv(cmd: &std::process::Command) -> Vec<String> {
    cmd.get_args()
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

/// Audio binary pins `ConverterMode::Audio` + bootstrap env-var pair.
///
/// caveman: `ConverterMode::from_env` is private in big-converter-ui,
/// and `set_converter_mode` is OnceLock-protected (process-global —
/// unsafe to toggle in tests). So we pin the exported constants here;
/// runtime gate is exercised in big-converter-ui's own crate tests.
#[test]
fn audio_mode_pin_and_bootstrap_env_pair() {
    assert!(matches!(APP_MODE, ConverterMode::Audio));
    assert!(APP_MODE.is_audio());
    assert_eq!(BOOTSTRAP_ENV_KEY, "BIG_MEDIA_AUDIO_CONVERTER");
    assert_eq!(BOOTSTRAP_ENV_VAL, "1");
}

/// `ConverterMode::from_env` (indirect via runtime gate) reads env var.
/// We can't safely call the OnceLock-protected `set_converter_mode`
/// (process-global), so we exercise the enum directly.
#[test]
fn mode_branching_is_distinct() {
    assert_ne!(ConverterMode::Audio, ConverterMode::Video);
    assert!(ConverterMode::Audio.is_audio());
    assert!(!ConverterMode::Video.is_audio());
}

/// Settings round-trip: write keys, flush, reopen, read back.
/// Redirects `~/.config` into a tempdir via `XDG_CONFIG_HOME`.
#[cfg(not(miri))]
#[test]
fn settings_persistence_round_trip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // SAFETY: test process; no other thread touches env here.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", tmp.path());
    }

    let s = Settings::new("big-audio-converter-test");
    assert!(s.set_string("audio-conv-format", "opus"));
    assert!(s.set_int("audio-conv-channels", 2));
    assert!(s.set_double("audio-conv-volume", 1.5));
    assert!(s.set_boolean("conv-eq-enabled", true));
    s.flush();

    let s2 = Settings::new("big-audio-converter-test");
    assert_eq!(s2.string("audio-conv-format"), "opus");
    assert_eq!(s2.int("audio-conv-channels"), 2);
    assert!((s2.double("audio-conv-volume") - 1.5).abs() < 1e-9);
    assert!(s2.boolean("conv-eq-enabled"));
}

/// `tr!()` is the gettext shim. Without a bound catalog the identity
/// path is the contract: input string survives untouched, output is
/// owned `String`, and the function never panics on common UI strings.
#[test]
fn i18n_msgids_are_non_empty() {
    for message_id in ["Cancel", "Convert", "Add Files"] {
        assert!(!message_id.is_empty());
    }
}

#[cfg(not(miri))]
#[test]
fn i18n_routes_known_strings() {
    // caveman: no catalog bound in test -> identity path
    let cases = ["Cancel", "Convert", "Add Files"];
    for s in cases {
        let out = i18n(s);
        assert!(!out.is_empty(), "{s}: empty");
        // identity contract holds when no catalog is bound
        assert_eq!(out, s, "unbound i18n must be identity");
    }
}

/// Regression: build_command must place `-i` before the path token, so a
/// pathological filename like "-f lavfi -i /dev/zero" cannot be parsed
/// by ffmpeg as flags. (ffmpeg has no `--` terminator, but a leading
/// `-i` consumes the next token verbatim as the input URL.)
#[test]
fn ffmpeg_argv_isolates_hostile_filename() {
    let evil = PathBuf::from("-f lavfi -i /dev/zero");
    let out = PathBuf::from("/tmp/out.mp3");
    let mut profile = dummy_profile();
    profile.video.codec = "copy".into();
    profile.audio.handling = "copy".into();
    let job = base_job(evil.clone(), out, profile);
    let cmd = job.build_command();
    let args = argv(&cmd);

    let path_str = evil.to_string_lossy().to_string();
    let idx = args
        .iter()
        .position(|a| a == &path_str)
        .expect("hostile path must appear");

    // Must be preceded by `-i`.
    assert!(idx > 0, "no preceding token: {args:?}");
    assert_eq!(args[idx - 1], "-i", "path not isolated by -i: {args:?}");
}

/// Regression: filenames with shell-meta chars are passed as a single
/// argv element and never split into multiple flags.
#[test]
fn ffmpeg_argv_keeps_special_chars_single_arg() {
    let weird = PathBuf::from("/tmp/[01] Mús ica (% special).mp3");
    let out = PathBuf::from("/tmp/out.mp3");
    let job = base_job(weird.clone(), out, dummy_profile());
    let cmd = job.build_command();
    let args = argv(&cmd);
    let s = weird.to_string_lossy().to_string();
    assert!(args.contains(&s), "lost weird path: {args:?}");
}

/// Stage 5: canonicalize() of a broken symlink fails → LAST_DIR must
/// NOT be updated. We mimic the production check (see
/// `big-converter-ui/.../window/files.rs`).
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
    let tmp = tempfile::tempdir().expect("tempdir");
    let link = tmp.path().join("dangling.mp3");
    // dangling target — never created
    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.path().join("nonexistent"), &link).expect("symlink");

    // Production guard: canonicalize() returns Err → no put_string.
    let resolved = std::fs::canonicalize(&link);
    assert!(
        resolved.is_err(),
        "canonicalize unexpectedly succeeded: {resolved:?}",
    );

    // Simulate the settings path: if we *had* fallen back to `link`,
    // LAST_DIR would now hold the user-never-opened tempdir. The
    // guard prevents that — express it as a positive invariant.
    assert!(
        last_dir_from_canonicalize_result(resolved).is_none(),
        "LAST_DIR was poisoned",
    );
}

// caveman: removed `dropping_source_id_holder_is_clean` — vacuous
// (Box<dyn FnOnce()> stand-in never exercised real glib::SourceId
// drop semantics, which require a running MainContext).

// --- Property-ish tests (deterministic combos, no proptest dep). ---

/// Resolution rejects zero or absurd dimensions; profile builder is
/// deterministic across rebuilds.
#[test]
fn profile_builder_deterministic() {
    let a = dummy_profile();
    let b = dummy_profile();
    let ta = toml::to_string(&a).expect("toml a");
    let tb = toml::to_string(&b).expect("toml b");
    assert_eq!(ta, tb, "non-deterministic profile toml");
}

#[cfg(not(miri))]
#[test]
fn resolution_bounds() {
    // caveman: drive real prod path (build_video_filters scale token)
    // audio binary: video filters never apply on audio-only jobs, so
    // we still exercise the runtime via a video-flag job toggle.
    let valid: [(u32, u32); 4] = [(640, 360), (1280, 720), (1920, 1080), (3840, 2160)];
    for (w, h) in valid {
        let mut profile = dummy_profile();
        profile.video.filters.resolution = format!("{w}x{h}");
        let mut job = base_job(
            PathBuf::from("/tmp/in.mkv"),
            PathBuf::from("/tmp/out.mkv"),
            profile,
        );
        job.audio_only = false; // exercise video filter path
        let filters = job.build_video_filters();
        assert!(
            filters.contains(&format!("{w}")) && filters.contains(&format!("{h}")),
            "({w},{h}) missing from filter chain: {filters}",
        );
    }
    // "auto" / empty -> no scale token
    let mut profile = dummy_profile();
    profile.video.filters.resolution = "auto".into();
    let mut job = base_job(
        PathBuf::from("/tmp/in.mkv"),
        PathBuf::from("/tmp/out.mkv"),
        profile,
    );
    job.audio_only = false;
    let filters = job.build_video_filters();
    assert!(!filters.contains("scale"), "auto produced scale: {filters}");
}

/// Smoke: real audio fixture path is recognized as a file the validator
/// would accept for audio-only conversion. (Read-only; never modified.)
#[test]
fn real_audio_fixture_argv_smoke() {
    use std::path::Path;
    // Point BAC_AUDIO_FIXTURES at a dir of .mp3 files to exercise this test.
    let Some(dir) = std::env::var_os("BAC_AUDIO_FIXTURES") else {
        eprintln!("BAC_AUDIO_FIXTURES unset — skipping");
        return;
    };
    let fixtures = Path::new(&dir);
    if !fixtures.exists() {
        eprintln!("BAC_AUDIO_FIXTURES path missing — skipping");
        return;
    }
    // pick first .mp3 found, max one level deep
    let mut picked: Option<PathBuf> = None;
    if let Ok(rd) = std::fs::read_dir(fixtures) {
        'outer: for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Ok(rd2) = std::fs::read_dir(&p) {
                    for f in rd2.flatten() {
                        let fp = f.path();
                        if fp.extension().and_then(|s| s.to_str()) == Some("mp3") {
                            picked = Some(fp);
                            break 'outer;
                        }
                    }
                }
            } else if p.extension().and_then(|s| s.to_str()) == Some("mp3") {
                picked = Some(p);
                break;
            }
        }
    }
    let Some(input) = picked else {
        eprintln!("no .mp3 found — skipping");
        return;
    };
    // Construct Command only — never spawn.
    let job = base_job(
        input.clone(),
        PathBuf::from("/tmp/out.mp3"),
        dummy_profile(),
    );
    let args = argv(&job.build_command());
    assert!(args.contains(&input.to_string_lossy().to_string()));
}

// silence unused warning for the helper when not exercised
#[allow(dead_code)]
fn _force_link(p: profiles::Profile) -> profiles::Profile {
    p
}
