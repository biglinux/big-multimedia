//! Video-filter graph tests: rotation/flip → transpose, brightness/saturation
//! → eq=, crop, scale, GPU wrapping (Nvidia hwdownload/hwupload_cuda, Intel
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
    assert!(vf.ends_with("hwupload_cuda"), "got: {vf}");
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
    assert_eq!(job.resolve_encoder("vp9"), "vp9_vaapi");
    job.gpu.gpu_type = GpuType::Cpu;
    assert_eq!(job.resolve_encoder("vp9"), "libvpx-vp9");
}

#[test]
fn intel_full_gpu_wraps_with_qsv() {
    let vf = gpu_job(
        profiles::VideoFilters {
            flip_h: true,
            ..profiles::VideoFilters::default()
        },
        GpuStage::FullGpu,
        GpuType::Intel,
    )
    .build_video_filters();
    assert!(
        vf.contains("hwupload=extra_hw_frames=64,format=qsv"),
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
    assert!(vf.contains("hwupload_cuda"), "got: {vf}");
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
fn color_eq_fires_on_each_axis_independently() {
    // Each color axis alone must enable the eq filter (pins the three `> threshold`
    // comparisons in `has_color`; a `==` mutation would drop the eq when only that
    // axis is non-zero).
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
    assert!(
        with(profiles::VideoFilters {
            hue: 0.5,
            ..profiles::VideoFilters::default()
        })
        .contains("hue=0.5"),
        "hue alone"
    );
    // Fully neutral → no eq filter at all.
    assert!(!with(profiles::VideoFilters::default()).contains("eq="));
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

    // Non-av1 software with auto pixel format → no -pix_fmt.
    let mut p2 = dummy_profile();
    p2.video.codec = "h264".into();
    p2.video.pixel_format = "auto".into();
    let mut j2 = base_job(p2);
    j2.stage = GpuStage::Software;
    let mut cmd2 = std::process::Command::new("ffmpeg");
    j2.add_video_args(&mut cmd2);
    assert!(
        !args(&cmd2).iter().any(|a| a == "-pix_fmt"),
        "auto = no pix_fmt"
    );

    // Explicit non-auto pixel format → emitted.
    let mut p3 = dummy_profile();
    p3.video.codec = "h264".into();
    p3.video.pixel_format = "yuv444p".into();
    let mut j3 = base_job(p3);
    j3.stage = GpuStage::Software;
    let mut cmd3 = std::process::Command::new("ffmpeg");
    j3.add_video_args(&mut cmd3);
    assert!(
        args(&cmd3).contains(&"yuv444p".to_string()),
        "explicit pix_fmt"
    );
}
