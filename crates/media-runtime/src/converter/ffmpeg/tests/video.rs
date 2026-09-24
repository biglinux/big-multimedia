//! Video-filter graph tests: rotation/flip → transpose, brightness/saturation
//! → eq=, crop, scale, GPU wrapping (Nvidia hwdownload/hwupload, Intel
//! format=nv12,hwupload, vaapi scale_vaapi). Saturation calibration is
//! pinned via characterization tests because the preview-to-ffmpeg ratio
//! (1.0 + 0.5*excess) must stay stable across refactors.

use super::*;

#[test]
fn test_video_filter_rotation_90() {
    let job = job_with_filters(
        profiles::VideoFilters {
            rotation: 90,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    assert_eq!(job.build_video_filters(), "transpose=1");
}

#[test]
fn test_video_filter_rotation_180() {
    let job = job_with_filters(
        profiles::VideoFilters {
            rotation: 180,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    assert_eq!(job.build_video_filters(), "transpose=1,transpose=1");
}

#[test]
fn test_video_filter_rotation_270() {
    let job = job_with_filters(
        profiles::VideoFilters {
            rotation: 270,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    assert_eq!(job.build_video_filters(), "transpose=2");
}

#[test]
fn test_video_filter_flip_h() {
    let job = job_with_filters(
        profiles::VideoFilters {
            flip_h: true,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    assert_eq!(job.build_video_filters(), "hflip");
}

#[test]
fn test_video_filter_flip_v() {
    let job = job_with_filters(
        profiles::VideoFilters {
            flip_v: true,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    assert_eq!(job.build_video_filters(), "vflip");
}

#[test]
fn test_video_filter_both_flips() {
    let job = job_with_filters(
        profiles::VideoFilters {
            flip_h: true,
            flip_v: true,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    assert_eq!(job.build_video_filters(), "hflip,vflip");
}

#[test]
fn test_video_filter_brightness() {
    let job = job_with_filters(
        profiles::VideoFilters {
            brightness: 0.5,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    let vf = job.build_video_filters();
    assert!(vf.contains("eq=brightness=0.5"), "got: {vf}");
}

#[test]
fn test_video_filter_contrast() {
    let job = job_with_filters(
        profiles::VideoFilters {
            contrast: 1.5,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    let vf = job.build_video_filters();
    assert!(vf.contains("contrast=1.5"), "got: {vf}");
}

// Below neutral (1.0) → 1:1 mapping.
#[test]
fn characterization_saturation_below_neutral() {
    let job = job_with_filters(
        profiles::VideoFilters {
            saturation: 0.5,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    let vf = job.build_video_filters();
    assert!(vf.contains("saturation=0.5"), "got: {vf}");
}

// Neutral (1.0) → no eq emitted.
#[test]
fn characterization_saturation_neutral_skipped() {
    let job = job_with_filters(profiles::VideoFilters::default(), GpuStage::Software);
    let vf = job.build_video_filters();
    assert!(
        !vf.contains("saturation"),
        "neutral saturation should not emit eq filter, got: {vf}"
    );
}

// Pinned: preview 2.0 → ffmpeg 2.5 (1.0 + 0.5 * excess). Refactor must not drift.
#[test]
fn characterization_saturation_above_neutral_2_0_to_2_5() {
    let job = job_with_filters(
        profiles::VideoFilters {
            saturation: 2.0,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    let vf = job.build_video_filters();
    assert!(
        vf.contains("saturation=2.5"),
        "preview saturation 2.0 must map to ffmpeg 2.5; got: {vf}"
    );
}

// Pinned: preview 1.5 → ffmpeg 1.75.
#[test]
fn characterization_saturation_above_neutral_1_5_to_1_75() {
    let job = job_with_filters(
        profiles::VideoFilters {
            saturation: 1.5,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    let vf = job.build_video_filters();
    assert!(
        vf.contains("saturation=1.75"),
        "preview saturation 1.5 must map to ffmpeg 1.75; got: {vf}"
    );
}

// Deadband: |s - 1.0| < 0.001 → skipped.
#[test]
fn characterization_saturation_within_threshold_skipped() {
    let job = job_with_filters(
        profiles::VideoFilters {
            saturation: 1.0005,
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    let vf = job.build_video_filters();
    assert!(
        !vf.contains("saturation"),
        "saturation within deadband should not emit; got: {vf}"
    );
}

#[test]
fn test_video_filter_crop() {
    let job = job_with_filters(
        profiles::VideoFilters {
            crop: Some(CropRect {
                w: 640,
                h: 480,
                x: 10,
                y: 20,
            }),
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    assert_eq!(job.build_video_filters(), "crop=640:480:10:20");
}

#[test]
fn test_video_filter_resolution_software() {
    let job = job_with_filters(
        profiles::VideoFilters {
            resolution: "1920x1080".into(),
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    assert_eq!(job.build_video_filters(), "scale=1920:1080");
}

#[test]
fn test_video_filter_combined() {
    let job = job_with_filters(
        profiles::VideoFilters {
            flip_h: true,
            brightness: 0.3,
            resolution: "1280x720".into(),
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    let vf = job.build_video_filters();
    assert!(vf.contains("hflip"), "got: {vf}");
    assert!(vf.contains("eq=brightness=0.3"), "got: {vf}");
    assert!(vf.contains("scale=1280:720"), "got: {vf}");
}

#[test]
fn test_video_filter_crop_override() {
    let mut job = job_with_filters(
        profiles::VideoFilters {
            crop: Some(CropRect {
                w: 100,
                h: 100,
                x: 0,
                y: 0,
            }),
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    );
    job.crop = Some(CropRect {
        w: 200,
        h: 200,
        x: 5,
        y: 5,
    });
    assert_eq!(job.build_video_filters(), "crop=200:200:5:5");
}

#[test]
fn test_video_filter_full_gpu_nvidia_wrapping() {
    let mut job = job_with_filters(
        profiles::VideoFilters {
            flip_h: true,
            ..profiles::VideoFilters::default()
        },
        GpuStage::FullGpu,
    );
    job.gpu = GpuDevice {
        gpu_type: GpuType::Nvidia,
        name: "RTX".into(),
        device_path: String::new(),
    };
    let vf = job.build_video_filters();
    assert!(vf.starts_with("hwdownload,format=nv12,"), "got: {vf}");
    assert!(vf.contains("hflip"), "got: {vf}");
    assert!(vf.ends_with("hwupload"), "got: {vf}");
}

#[test]
fn test_video_filter_partial_gpu_upload() {
    let mut job = job_with_filters(
        profiles::VideoFilters {
            flip_v: true,
            ..profiles::VideoFilters::default()
        },
        GpuStage::PartialGpu,
    );
    job.gpu = GpuDevice {
        gpu_type: GpuType::Intel,
        name: "Intel".into(),
        device_path: String::new(),
    };
    let vf = job.build_video_filters();
    assert!(vf.contains("vflip"), "got: {vf}");
    assert!(vf.contains("format=nv12,hwupload"), "got: {vf}");
}

#[test]
fn test_video_filter_full_gpu_nvidia_scale() {
    let mut job = job_with_filters(
        profiles::VideoFilters {
            resolution: "1920x1080".into(),
            ..profiles::VideoFilters::default()
        },
        GpuStage::FullGpu,
    );
    job.gpu = GpuDevice {
        gpu_type: GpuType::Nvidia,
        name: "RTX".into(),
        device_path: String::new(),
    };
    let vf = job.build_video_filters();
    assert!(vf.contains("scale_cuda=1920:1080"), "got: {vf}");
}

#[test]
fn test_video_filter_full_gpu_vaapi_scale() {
    let mut job = job_with_filters(
        profiles::VideoFilters {
            resolution: "1280x720".into(),
            ..profiles::VideoFilters::default()
        },
        GpuStage::FullGpu,
    );
    job.gpu = GpuDevice {
        gpu_type: GpuType::Amd,
        name: "RX".into(),
        device_path: String::new(),
    };
    let vf = job.build_video_filters();
    assert!(vf.contains("scale_vaapi=w=1280:h=720"), "got: {vf}");
}

fn gpu_job(filters: profiles::VideoFilters, stage: GpuStage, gpu_type: GpuType) -> FfmpegJob {
    let mut job = job_with_filters(filters, stage);
    job.gpu = GpuDevice {
        gpu_type,
        name: "gpu".into(),
        device_path: String::new(),
    };
    job
}

#[test]
fn resolve_encoder_hardware_vp9_and_prores() {
    let mut job = job_with_filters(profiles::VideoFilters::default(), GpuStage::FullGpu);
    job.gpu = GpuDevice {
        gpu_type: GpuType::Intel,
        name: "i".into(),
        device_path: String::new(),
    };
    assert_eq!(job.resolve_encoder("vp9"), "vp9_qsv");
    assert_eq!(job.resolve_encoder("prores"), "prores_ks");
    job.gpu.gpu_type = GpuType::Nvidia;
    assert_eq!(job.resolve_encoder("vp9"), "libvpx-vp9");
    job.gpu.gpu_type = GpuType::Cpu;
    assert_eq!(job.resolve_encoder("vp9"), "libvpx-vp9");
}

#[test]
fn intel_full_gpu_wraps_with_qsv() {
    let vf = gpu_job(
        profiles::VideoFilters {
            flip_h: true,
            hue: 0.5,
            ..profiles::VideoFilters::default()
        },
        GpuStage::FullGpu,
        GpuType::Intel,
    )
    .build_video_filters();
    assert!(
        vf.contains("hue=h=-90,format=nv12,hwupload=extra_hw_frames=64,format=qsv"),
        "got: {vf}"
    );
}

#[test]
fn intel_full_gpu_scale_uses_qsv() {
    let vf = gpu_job(
        profiles::VideoFilters {
            resolution: "1280x720".into(),
            ..profiles::VideoFilters::default()
        },
        GpuStage::FullGpu,
        GpuType::Intel,
    )
    .build_video_filters();
    assert!(vf.contains("scale_qsv=w=1280:h=720"), "got: {vf}");
}

#[test]
fn intel_partial_gpu_uploads_qsv() {
    let vf = gpu_job(
        profiles::VideoFilters {
            flip_v: true,
            ..profiles::VideoFilters::default()
        },
        GpuStage::PartialGpu,
        GpuType::Intel,
    )
    .build_video_filters();
    assert!(
        vf.contains("hwupload=extra_hw_frames=64,format=qsv"),
        "got: {vf}"
    );
}

#[test]
fn nvidia_partial_gpu_uploads_cuda() {
    let vf = gpu_job(
        profiles::VideoFilters {
            flip_v: true,
            ..profiles::VideoFilters::default()
        },
        GpuStage::PartialGpu,
        GpuType::Nvidia,
    )
    .build_video_filters();
    assert!(vf.contains("hwupload"), "got: {vf}");
}

#[test]
fn partial_gpu_uploads_even_without_visual_effects() {
    for gpu in [GpuType::Nvidia, GpuType::Intel, GpuType::Amd] {
        let vf = gpu_job(profiles::VideoFilters::default(), GpuStage::PartialGpu, gpu)
            .build_video_filters();
        assert!(
            vf.contains("hwupload"),
            "{gpu:?} receives CPU frames without an upload: {vf}"
        );
    }
}

#[test]
fn partial_gpu_scale_is_vendor_specific() {
    let nvidia = gpu_job(
        profiles::VideoFilters {
            resolution: "1920x1080".into(),
            ..profiles::VideoFilters::default()
        },
        GpuStage::PartialGpu,
        GpuType::Nvidia,
    )
    .build_video_filters();
    assert!(nvidia.contains("scale_cuda=1920:1080"), "got: {nvidia}");

    let intel = gpu_job(
        profiles::VideoFilters {
            resolution: "1920x1080".into(),
            ..profiles::VideoFilters::default()
        },
        GpuStage::PartialGpu,
        GpuType::Intel,
    )
    .build_video_filters();
    assert!(intel.contains("scale_qsv=w=1920:h=1080"), "got: {intel}");
}

#[test]
fn color_filters_apply_each_axis_independently() {
    let with =
        |f: profiles::VideoFilters| job_with_filters(f, GpuStage::Software).build_video_filters();
    assert!(
        with(profiles::VideoFilters {
            brightness: 0.5,
            ..profiles::VideoFilters::default()
        })
        .contains("eq=brightness=0.5"),
        "brightness alone"
    );
    assert!(
        with(profiles::VideoFilters {
            contrast: 1.5,
            ..profiles::VideoFilters::default()
        })
        .contains("contrast=1.5"),
        "contrast alone"
    );
    assert!(
        with(profiles::VideoFilters {
            saturation: 1.8,
            ..profiles::VideoFilters::default()
        })
        .contains("eq="),
        "saturation alone"
    );
    assert_eq!(
        with(profiles::VideoFilters {
            hue: 0.5,
            ..profiles::VideoFilters::default()
        }),
        "hue=h=-90",
        "hue alone"
    );
    // Fully neutral → no eq filter at all.
    assert!(!with(profiles::VideoFilters::default()).contains("eq="));
}

#[test]
#[cfg(not(miri))]
#[ignore = "requires FFmpeg with the eq and hue filters"]
fn ffmpeg_color_adjustments_change_pixels() {
    use big_os_kit::subprocess::BigSubprocessSpec;
    use std::time::Duration;

    let neutral = profiles::VideoFilters::default();
    let cases = [
        ("neutral", neutral.clone()),
        (
            "brightness",
            profiles::VideoFilters {
                brightness: 0.2,
                ..neutral.clone()
            },
        ),
        (
            "contrast",
            profiles::VideoFilters {
                contrast: 1.5,
                ..neutral.clone()
            },
        ),
        (
            "grayscale",
            profiles::VideoFilters {
                saturation: 0.0,
                ..neutral.clone()
            },
        ),
        (
            "hue positive",
            profiles::VideoFilters {
                hue: 0.5,
                ..neutral.clone()
            },
        ),
        (
            "hue negative",
            profiles::VideoFilters {
                hue: -0.5,
                ..neutral.clone()
            },
        ),
        (
            "combined",
            profiles::VideoFilters {
                brightness: 0.2,
                hue: 0.5,
                ..neutral
            },
        ),
    ];
    for (name, filters) in cases {
        let graph = job_with_filters(filters, GpuStage::Software).build_video_filters();
        let output = BigSubprocessSpec::builder()
            .program("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=red:size=64x64,format=yuv444p",
                "-vf",
            ])
            .arg(if graph.is_empty() { "null" } else { &graph })
            .args([
                "-frames:v",
                "1",
                "-pix_fmt",
                "yuv444p",
                "-f",
                "rawvideo",
                "-",
            ])
            .timeout(Duration::from_secs(5))
            .build()
            .run()
            .unwrap();
        assert!(
            output.status.success(),
            "{name} ({graph}): {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout.len(), 64 * 64 * 3, "{name}");
        let actual = [output.stdout[0], output.stdout[4096], output.stdout[8192]];
        // Limited-range BT.601 red is YUV [81, 90, 240]. mpv's positive
        // quarter-turn maps (U-128,V-128) to (V-128,-(U-128)).
        let expected: [u8; 3] = match name {
            "neutral" => [81, 90, 240],
            "brightness" => [132, 90, 240],
            "contrast" => [58, 90, 240],
            "grayscale" => [81, 128, 128],
            "hue positive" => [81, 240, 166],
            "hue negative" => [81, 16, 90],
            "combined" => [132, 240, 166],
            _ => unreachable!(),
        };
        eprintln!("{name}: {graph}; YUV={actual:?}");
        assert!(
            actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2),
            "{name}: expected {expected:?}, got {actual:?}"
        );
    }
}

#[test]
fn scale_only_software_is_not_short_circuited_to_empty() {
    // No sw_filters but a resolution set: must still emit the scale filter
    // (pins `sw_filters.is_empty() && !has_scale`).
    let vf = job_with_filters(
        profiles::VideoFilters {
            resolution: "1920x1080".into(),
            ..profiles::VideoFilters::default()
        },
        GpuStage::Software,
    )
    .build_video_filters();
    assert_eq!(vf, "scale=1920:1080");
}

#[test]
fn pixel_format_args_select_av1_and_explicit_override() {
    // Software + av1 forces 10-bit pixel format.
    let mut profile = dummy_profile();
    profile.video.codec = "av1".into();
    let mut job = base_job(profile);
    job.stage = GpuStage::Software;
    let mut cmd = std::process::Command::new("ffmpeg");
    job.add_video_args(&mut cmd);
    assert!(
        args(&cmd).contains(&"yuv420p10le".to_string()),
        "av1 10-bit"
    );

    // Automatic H264 uses 8-bit 4:2:0; an explicit format remains unchanged.
    let mut p2 = dummy_profile();
    p2.video.codec = "h264".into();
    p2.video.pixel_format = "auto".into();
    let mut j2 = base_job(p2);
    j2.stage = GpuStage::Software;
    let mut cmd2 = std::process::Command::new("ffmpeg");
    j2.add_video_args(&mut cmd2);
    assert!(
        args(&cmd2).windows(2).any(|a| a == ["-pix_fmt", "yuv420p"]),
        "automatic H264 must use compatible 8-bit samples"
    );

    // Explicit non-auto pixel format → emitted.
    let mut p3 = dummy_profile();
    p3.video.codec = "av1".into();
    p3.video.pixel_format = "yuv420p".into();
    let mut j3 = base_job(p3);
    j3.stage = GpuStage::Software;
    let mut cmd3 = std::process::Command::new("ffmpeg");
    j3.add_video_args(&mut cmd3);
    assert!(
        args(&cmd3).windows(2).any(|a| a == ["-pix_fmt", "yuv420p"]),
        "explicit pix_fmt"
    );
}
