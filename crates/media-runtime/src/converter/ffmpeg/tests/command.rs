//! ffmpeg command tests: trim, track selection, mp4 +faststart, audio
//! codec mapping, subtitle inputs, NVENC / VAAPI / QSV hwaccel init,
//! per-codec software fallback args, and `ffmpeg_timestamp` boundary cases.

use super::*;

#[test]
fn test_trim_start_only() {
    let mut profile = dummy_profile();
    profile.video.codec = "copy".into();
    profile.audio.handling = "copy".into();
    let mut job = base_job(profile);
    job.trim = TrimRange {
        start: Some(10.0),
        end: None,
    };
    let args = args(&job.build_command());
    assert!(args.contains(&"-ss".into()), "missing -ss");
    assert!(args.contains(&"00:00:10.000".into()), "args: {args:?}");
}

#[test]
fn test_trim_start_and_end() {
    let mut profile = dummy_profile();
    profile.video.codec = "copy".into();
    profile.audio.handling = "copy".into();
    let mut job = base_job(profile);
    job.trim = TrimRange {
        start: Some(5.0),
        end: Some(15.0),
    };
    let args = args(&job.build_command());
    assert!(args.contains(&"-ss".into()));
    assert!(args.contains(&"-t".into()), "missing -t (duration)");
}

#[test]
fn test_audio_track_selection() {
    let mut profile = dummy_profile();
    profile.video.codec = "copy".into();
    let mut job = base_job(profile);
    job.audio_track = Some(2);
    let args = args(&job.build_command());
    assert!(args.contains(&"0:a:2?".into()), "args: {args:?}");
}

#[test]
fn test_video_track_selection() {
    let mut profile = dummy_profile();
    profile.video.codec = "copy".into();
    let mut job = base_job(profile);
    job.video_track = Some(1);
    let args = args(&job.build_command());
    assert!(args.contains(&"0:v:1".into()), "args: {args:?}");
}

#[test]
fn test_mp4_faststart() {
    let args = args(&base_job(dummy_profile()).build_command());
    assert!(
        args.contains(&"+faststart".into()),
        "mp4 should have +faststart, args: {args:?}"
    );
}

#[test]
fn test_audio_codec_opus() {
    let mut profile = dummy_profile();
    profile.audio.handling = "encode".into();
    profile.audio.codec = "opus".into();
    let args = args(&base_job(profile).build_command());
    assert!(args.contains(&"libopus".into()), "args: {args:?}");
}

#[test]
fn test_audio_no_audio() {
    let mut profile = dummy_profile();
    profile.audio.handling = "none".into();
    let args = args(&base_job(profile).build_command());
    assert!(args.contains(&"-an".into()), "args: {args:?}");
}

#[test]
fn test_external_subtitle_input_file() {
    let mut profile = dummy_profile();
    profile.subtitles.handling = "burn".into();
    let mut job = base_job(profile);
    job.subtitle = SubtitleInput::ExternalFile(PathBuf::from("/tmp/subs.srt"));
    let args = args(&job.build_command());
    let input_indices: Vec<_> = args
        .iter()
        .enumerate()
        .filter(|(_, arg)| *arg == "-i")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        input_indices.len(),
        2,
        "should have 2 -i args, args: {args:?}"
    );
}

#[test]
fn test_subtitle_embedded_codec_mp4() {
    let mut profile = dummy_profile();
    profile.subtitles.handling = "embedded".into();
    let mut job = base_job(profile);
    job.subtitle = SubtitleInput::EmbeddedCopy;
    let args = args(&job.build_command());
    assert!(
        args.contains(&"mov_text".into()),
        "mp4 embedded subs should use mov_text, args: {args:?}"
    );
}

#[test]
fn test_nvidia_hw_init_default_device() {
    let mut profile = dummy_profile();
    profile.video.codec = "h264".into();
    let mut job = base_job(profile);
    job.gpu = GpuDevice {
        gpu_type: GpuType::Nvidia,
        name: "RTX".into(),
        device_path: String::new(),
    };
    job.stage = GpuStage::FullGpu;
    let args = args(&job.build_command());
    assert!(args.contains(&"cuda=cu:0".into()), "args: {args:?}");
    assert!(args.windows(2).any(|a| a == ["-hwaccel_device", "cu"]));
    assert!(args.contains(&"-hwaccel".into()), "args: {args:?}");
}

#[test]
fn test_vaapi_hw_init_custom_device() {
    let mut profile = dummy_profile();
    profile.video.codec = "h264".into();
    let mut job = base_job(profile);
    job.gpu = GpuDevice {
        gpu_type: GpuType::Intel,
        name: "Intel".into(),
        device_path: "/dev/dri/renderD129".into(),
    };
    job.stage = GpuStage::PartialGpu;
    let args = args(&job.build_command());
    assert!(
        args.contains(&"vaapi=va:/dev/dri/renderD129".into()),
        "args: {args:?}"
    );
    assert!(
        args.iter().any(|arg| arg.contains("qsv=qs@va")),
        "args: {args:?}"
    );
    assert!(
        !args.contains(&"-hwaccel".into()),
        "partial should not have hwaccel, args: {args:?}"
    );
    assert!(args.windows(2).any(|a| a == ["-filter_hw_device", "qs"]));
}

#[test]
fn software_codecs_and_audio_do_not_initialize_a_video_gpu() {
    for (codec, audio_only) in [
        ("vp9", false),
        ("prores", false),
        ("copy", false),
        ("h264", true),
    ] {
        let mut profile = dummy_profile();
        profile.video.codec = codec.into();
        let mut job = base_job(profile);
        job.gpu = GpuDevice {
            gpu_type: GpuType::Nvidia,
            name: "NVIDIA".into(),
            device_path: "1".into(),
        };
        job.audio_only = audio_only;
        for stage in [GpuStage::FullGpu, GpuStage::PartialGpu, GpuStage::Software] {
            job.stage = stage;
            let a = args(&job.build_command());
            assert!(
                !a.iter().any(|a| a == "-init_hw_device" || a == "-cq"),
                "{codec} audio={audio_only} {stage:?}: {a:?}"
            );
            if !audio_only && codec == "vp9" {
                assert_eq!(value_after(&a, "-c:v").as_deref(), Some("libvpx-vp9"));
            }
            if !audio_only && codec == "prores" {
                assert_eq!(value_after(&a, "-c:v").as_deref(), Some("prores_ks"));
            }
        }
    }
}

#[test]
fn nvidia_selected_device_is_shared_by_decode_and_filter_upload() {
    let mut profile = dummy_profile();
    profile.video.codec = "h264".into();
    profile.video.filters.flip_h = true;
    let mut job = base_job(profile);
    job.gpu = GpuDevice {
        gpu_type: GpuType::Nvidia,
        name: "Second NVIDIA GPU".into(),
        device_path: "1".into(),
    };
    for stage in [GpuStage::FullGpu, GpuStage::PartialGpu] {
        job.stage = stage;
        let args = args(&job.build_command());
        assert!(args.contains(&"cuda=cu:1".into()), "{args:?}");
        assert!(args.windows(2).any(|a| a == ["-gpu", "1"]), "{args:?}");
        assert!(
            args.windows(2).any(|a| a == ["-filter_hw_device", "cu"]),
            "{args:?}"
        );
        assert!(!job.build_video_filters().contains("hwupload_cuda"));
    }
}

#[test]
#[cfg(all(not(miri), feature = "ui-core"))]
#[ignore = "run with CUDA_VISIBLE_DEVICES= on a CUDA host"]
fn cuda_visibility_mask_is_preserved_in_cast_sandbox() {
    use big_os_kit::subprocess::BigSubprocessSpec;
    use std::time::Duration;

    assert_eq!(std::env::var("CUDA_VISIBLE_DEVICES").as_deref(), Ok(""));
    let mut args = crate::cast::sandbox::options(&[], &[]).expect("bubblewrap required");
    args.extend(
        [
            "ffmpeg",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-init_hw_device",
            "cuda=cu:0",
            "-f",
            "lavfi",
            "-i",
            "color=size=640x360",
            "-frames:v",
            "1",
            "-f",
            "null",
            "-",
        ]
        .into_iter()
        .map(str::to_string),
    );
    let result = BigSubprocessSpec::builder()
        .program("bwrap")
        .args(args)
        .timeout(Duration::from_secs(10))
        .build()
        .run()
        .unwrap();
    assert!(
        !result.status.success(),
        "sandbox exposed CUDA despite the empty device mask"
    );
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(
        error.contains("CUDA_ERROR_NO_DEVICE"),
        "unexpected failure: {error}"
    );
}

#[test]
#[cfg(all(not(miri), feature = "ui-core"))]
#[ignore = "requires a usable GPU encoder, FFmpeg and bubblewrap"]
fn hardware_conversion_preserves_selected_device_and_applies_flip() {
    use big_os_kit::subprocess::BigSubprocessSpec;
    use std::time::Duration;

    let gpus: Vec<_> = crate::cast::gpu::detect_all_gpus()
        .into_iter()
        .filter(|g| g.gpu_type != GpuType::Cpu)
        .collect();
    assert!(
        !gpus.is_empty(),
        "this hardware test needs a working H264 GPU encoder"
    );
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.mp4");
    let output_dir = temp.path().join("output");
    std::fs::create_dir(&output_dir).unwrap();
    let generated = BigSubprocessSpec::builder().program("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=red:size=640x360:duration=0.1:rate=30,drawbox=x=320:y=0:w=320:h=360:color=blue:t=fill", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&input).timeout(Duration::from_secs(10)).build().run().unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );

    for gpu in gpus {
        eprintln!("testing {} at {}", gpu.name, gpu.device_path);
        for stage in [GpuStage::FullGpu, GpuStage::PartialGpu] {
            for flip in [false, true] {
                let mut profile = dummy_profile();
                profile.video.codec = "h264".into();
                profile.video.filters.flip_h = flip;
                if flip {
                    profile.video.filters.brightness = 0.05;
                }
                profile.output.container = "mp4".into();
                let mut job = base_job(profile);
                job.gpu = gpu.clone();
                job.stage = stage;
                job.input = input.clone();
                job.output = output_dir.join("encoded.mp4");
                let command = job.build_command();
                // The partial path also proves that CUDA visibility and GPU
                // nodes survive the real cast sandbox.
                let (program, arguments) = if stage == GpuStage::PartialGpu {
                    let mut arguments = crate::cast::sandbox::options(&[&input], &[&output_dir])
                        .expect("bubblewrap required");
                    arguments.push("ffmpeg".into());
                    arguments.extend(args(&command));
                    ("bwrap", arguments)
                } else {
                    ("ffmpeg", args(&command))
                };
                let encoded = BigSubprocessSpec::builder()
                    .program(program)
                    .args(arguments)
                    .timeout(Duration::from_secs(15))
                    .build()
                    .run()
                    .unwrap();
                assert!(
                    encoded.status.success(),
                    "{stage:?} flip={flip}: {}",
                    String::from_utf8_lossy(&encoded.stderr)
                );
                let decoded = BigSubprocessSpec::builder()
                    .program("ffmpeg")
                    .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
                    .arg(&job.output)
                    .args(["-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "-"])
                    .timeout(Duration::from_secs(10))
                    .build()
                    .run()
                    .unwrap();
                assert!(
                    decoded.status.success(),
                    "{}",
                    String::from_utf8_lossy(&decoded.stderr)
                );
                assert_eq!(decoded.stdout.len(), 640 * 360 * 3);
                let left = (180 * 640 + 160) * 3;
                let right = (180 * 640 + 480) * 3;
                let (red, blue) = if flip { (right, left) } else { (left, right) };
                assert!(decoded.stdout[red] > 200 && decoded.stdout[red + 2] < 30);
                assert!(decoded.stdout[blue + 2] > 200 && decoded.stdout[blue] < 30);
            }
        }
    }
}

#[test]
fn test_software_h265_args() {
    let mut profile = dummy_profile();
    profile.video.codec = "h265".into();
    let args = args(&base_job(profile).build_command());
    assert!(args.contains(&"libx265".into()), "args: {args:?}");
    assert!(args.contains(&"-crf".into()), "args: {args:?}");
    assert!(
        args.iter().any(|arg| arg.contains("log-level=warning")),
        "args: {args:?}"
    );
}

#[test]
fn test_software_vp9_args() {
    let mut profile = dummy_profile();
    profile.video.codec = "vp9".into();
    let mut job = base_job(profile);
    job.output = PathBuf::from("/tmp/out.webm");
    let args = args(&job.build_command());
    assert!(args.contains(&"libvpx-vp9".into()), "args: {args:?}");
    assert!(args.contains(&"-row-mt".into()), "args: {args:?}");
    assert!(args.contains(&"-b:v".into()), "args: {args:?}");
}

#[test]
fn test_software_prores_args() {
    let mut profile = dummy_profile();
    profile.video.codec = "prores".into();
    profile.video.quality = "high".into();
    let mut job = base_job(profile);
    job.output = PathBuf::from("/tmp/out.mov");
    let args = args(&job.build_command());
    assert!(args.contains(&"prores_ks".into()), "args: {args:?}");
    assert!(args.contains(&"-vendor".into()), "args: {args:?}");
    assert!(args.contains(&"3".into()), "HQ profile=3, args: {args:?}");
}

#[test]
fn test_software_av1_pix_fmt() {
    let mut profile = dummy_profile();
    profile.video.codec = "av1".into();
    let mut job = base_job(profile);
    job.output = PathBuf::from("/tmp/out.mkv");
    let args = args(&job.build_command());
    assert!(
        args.contains(&"yuv420p10le".into()),
        "AV1 software should use 10-bit, args: {args:?}"
    );
}

#[test]
fn test_ffmpeg_timestamp_negative() {
    assert_eq!(ffmpeg_timestamp(-5.0), "00:00:00.000");
}

#[test]
fn test_ffmpeg_timestamp_large() {
    assert_eq!(ffmpeg_timestamp(9045.123), "02:30:45.123");
}

/// First argument value following `flag` in the built argv.
fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

#[test]
fn trim_duration_value_is_end_minus_start() {
    let mut profile = dummy_profile();
    profile.video.codec = "copy".into();
    profile.audio.handling = "copy".into();
    let mut job = base_job(profile);
    job.trim = TrimRange {
        start: Some(5.0),
        end: Some(15.0),
    };
    let a = args(&job.build_command());
    // duration = 15 - 5 = 10s; pins the subtraction against +/ mutants.
    assert_eq!(value_after(&a, "-t").as_deref(), Some("00:00:10.000"));
}

#[test]
fn trim_zero_duration_emits_no_t() {
    let mut profile = dummy_profile();
    profile.video.codec = "copy".into();
    profile.audio.handling = "copy".into();
    let mut job = base_job(profile);
    job.trim = TrimRange {
        start: Some(10.0),
        end: Some(10.0),
    };
    let a = args(&job.build_command());
    // duration 0 is not > 0 → no -t (pins the `> 0.0` guard against `>=`).
    assert!(!a.contains(&"-t".to_string()), "args: {a:?}");
}

#[test]
fn audio_handling_copy_uses_stream_copy() {
    // dummy_profile defaults audio.handling to "copy".
    let job = base_job(dummy_profile());
    let a = args(&job.build_command());
    assert_eq!(value_after(&a, "-c:a").as_deref(), Some("copy"));
}

#[test]
fn audio_aac_reencode_adds_coder_and_profile() {
    let mut profile = dummy_profile();
    profile.audio.handling = "convert".into();
    profile.audio.codec = "aac".into();
    let a = args(&base_job(profile).build_command());
    assert_eq!(value_after(&a, "-c:a").as_deref(), Some("aac"));
    assert!(a.contains(&"-aac_coder".to_string()), "args: {a:?}");
}

#[test]
fn audio_bitrate_present_for_lossy_absent_for_flac() {
    let mut lossy = dummy_profile();
    lossy.audio.handling = "convert".into();
    lossy.audio.codec = "mp3".into();
    lossy.audio.bitrate = "192k".into();
    let a = args(&base_job(lossy).build_command());
    assert_eq!(value_after(&a, "-b:a").as_deref(), Some("192k"));

    let mut lossless = dummy_profile();
    lossless.audio.handling = "convert".into();
    lossless.audio.codec = "flac".into();
    let a2 = args(&base_job(lossless).build_command());
    assert!(
        !a2.contains(&"-b:a".to_string()),
        "flac must not set bitrate: {a2:?}"
    );
}

#[test]
fn subtitle_burn_handling_disables_subtitles() {
    let mut profile = dummy_profile();
    profile.subtitles.handling = "burn".into();
    let a = args(&base_job(profile).build_command());
    assert!(a.contains(&"-sn".to_string()), "args: {a:?}");
}

#[test]
fn embedded_subtitles_mp4_maps_only_non_bitmap_streams() {
    let mut profile = dummy_profile();
    profile.subtitles.handling = "embedded".into();
    let mut job = base_job(profile); // output is .mp4
    job.subtitle_streams = vec![
        SubtitleStream {
            index: 2,
            codec: "hdmv_pgs_subtitle".into(),
            language: String::new(),
            title: String::new(),
            is_bitmap: true,
        },
        SubtitleStream {
            index: 3,
            codec: "subrip".into(),
            language: String::new(),
            title: String::new(),
            is_bitmap: false,
        },
    ];
    let a = args(&job.build_command());
    assert!(a.contains(&"0:3".to_string()), "non-bitmap mapped: {a:?}");
    assert!(!a.contains(&"0:2".to_string()), "bitmap not mapped: {a:?}");
}

#[test]
fn embedded_subtitles_mp4_without_streams_uses_optional_map() {
    let mut profile = dummy_profile();
    profile.subtitles.handling = "embedded".into();
    let job = base_job(profile); // empty subtitle_streams
    let a = args(&job.build_command());
    assert!(a.contains(&"0:s?".to_string()), "args: {a:?}");
}

#[test]
fn audio_only_skips_subtitle_mapping_and_args() {
    let mut profile = dummy_profile();
    profile.subtitles.handling = "embedded".into();
    let mut job = base_job(profile);
    job.audio_only = true;
    let a = args(&job.build_command());
    assert!(!a.contains(&"0:s?".to_string()), "no sub map: {a:?}");
    assert!(!a.contains(&"-sn".to_string()), "no -sn: {a:?}");
}

#[test]
fn crop_produces_video_filter() {
    let mut job = base_job(dummy_profile());
    job.crop = Some(CropRect {
        w: 100,
        h: 80,
        x: 10,
        y: 20,
    });
    let a = args(&job.build_command());
    assert_eq!(value_after(&a, "-vf").as_deref(), Some("crop=100:80:10:20"));
}

#[test]
fn audio_effect_emits_af_filter() {
    let mut profile = dummy_profile();
    profile.audio.handling = "convert".into();
    profile.audio.effects.hpf_enabled = true;
    profile.audio.effects.hpf_frequency = 100;
    let a = args(&base_job(profile).build_command());
    // A non-empty audio filter graph must be attached via -af.
    assert!(a.contains(&"-af".to_string()), "args: {a:?}");
    assert!(
        a.iter().any(|x| x.contains("highpass=f=100")),
        "args: {a:?}"
    );
}

#[test]
fn effective_duration_uses_trimmed_span_not_whole_media() {
    // A cut of [10, 15] of a 100s video processes 5s — the percent driver must
    // be 5.0, not 100.0 (the bug: progress finished at 5% and reported "done").
    let range = TrimRange {
        start: Some(10.0),
        end: Some(15.0),
    };
    assert!((range.effective_duration(100.0) - 5.0).abs() < f64::EPSILON);
}

#[test]
fn effective_duration_open_ended_falls_back_to_media_bounds() {
    // No trim → whole media.
    let whole = TrimRange::default();
    assert!((whole.effective_duration(100.0) - 100.0).abs() < f64::EPSILON);
    // Start only → from start to EOF.
    let from_start = TrimRange {
        start: Some(30.0),
        end: None,
    };
    assert!((from_start.effective_duration(100.0) - 70.0).abs() < f64::EPSILON);
    // End only → from 0 to end.
    let to_end = TrimRange {
        start: None,
        end: Some(40.0),
    };
    assert!((to_end.effective_duration(100.0) - 40.0).abs() < f64::EPSILON);
}

#[test]
fn effective_duration_clamps_to_media_and_guards_unknown_duration() {
    // End past the media length clamps to the media length.
    let over = TrimRange {
        start: Some(90.0),
        end: Some(120.0),
    };
    assert!((over.effective_duration(100.0) - 10.0).abs() < f64::EPSILON);
    // Unknown media duration (0.0) with an open end yields 0.0 (parser then
    // leaves percent unset instead of dividing by zero).
    let unknown = TrimRange {
        start: Some(5.0),
        end: None,
    };
    assert!(unknown.effective_duration(0.0).abs() < f64::EPSILON);
}
