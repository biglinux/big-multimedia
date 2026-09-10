//! Encoder selection + quality-table tests across NVENC/QSV/VAAPI/software.

use super::*;

#[test]
fn test_ffmpeg_timestamp() {
    assert_eq!(ffmpeg_timestamp(3661.5), "01:01:01.500");
    assert_eq!(ffmpeg_timestamp(0.0), "00:00:00.000");
}

#[test]
fn test_video_copy_produces_minimal_args() {
    let mut profile = dummy_profile();
    profile.video.codec = "copy".into();
    let cmd = base_job(profile).build_command();
    let args = args(&cmd);
    assert!(args.contains(&"-c:v".into()));
    assert!(args.contains(&"copy".into()));
}

#[test]
fn test_no_eq_filter_with_default_values() {
    let vf = base_job(dummy_profile()).build_video_filters();
    assert!(
        vf.is_empty(),
        "No video filters should be generated with default profile, got: {vf}"
    );
}

#[test]
fn test_nvidia_quality_tables() {
    assert_eq!(nvidia_cq("h264", "good"), 28);
    assert_eq!(nvidia_cq("h265", "veryhigh"), 22);
    assert_eq!(nvidia_preset_num("balanced"), 4);
    assert_eq!(nvidia_preset_num("veryfast"), 1);
}

#[test]
fn test_resolve_encoder_software_stage() {
    let mut job = base_job(dummy_profile());
    job.gpu = GpuDevice {
        gpu_type: GpuType::Nvidia,
        name: "RTX".into(),
        device_path: String::new(),
    };
    job.stage = GpuStage::Software;
    assert_eq!(job.resolve_encoder("h264"), "libx264");
    assert_eq!(job.resolve_encoder("h265"), "libx265");
    assert_eq!(job.resolve_encoder("av1"), "libsvtav1");
    assert_eq!(job.resolve_encoder("vp9"), "libvpx-vp9");
    assert_eq!(job.resolve_encoder("prores"), "prores_ks");
}

#[test]
fn test_resolve_encoder_nvidia_full_gpu() {
    let mut job = base_job(dummy_profile());
    job.gpu = GpuDevice {
        gpu_type: GpuType::Nvidia,
        name: "RTX".into(),
        device_path: String::new(),
    };
    job.stage = GpuStage::FullGpu;
    assert_eq!(job.resolve_encoder("h264"), "h264_nvenc");
    assert_eq!(job.resolve_encoder("h265"), "hevc_nvenc");
    assert_eq!(job.resolve_encoder("av1"), "av1_nvenc");
}

#[test]
fn test_resolve_encoder_intel() {
    let mut job = base_job(dummy_profile());
    job.gpu = GpuDevice {
        gpu_type: GpuType::Intel,
        name: "Arc".into(),
        device_path: String::new(),
    };
    job.stage = GpuStage::FullGpu;
    assert_eq!(job.resolve_encoder("h264"), "h264_qsv");
    assert_eq!(job.resolve_encoder("h265"), "hevc_qsv");
    assert_eq!(job.resolve_encoder("av1"), "av1_qsv");
}

#[test]
fn test_resolve_encoder_amd() {
    let mut job = base_job(dummy_profile());
    job.gpu = GpuDevice {
        gpu_type: GpuType::Amd,
        name: "RX".into(),
        device_path: String::new(),
    };
    job.stage = GpuStage::PartialGpu;
    assert_eq!(job.resolve_encoder("h264"), "h264_vaapi");
    assert_eq!(job.resolve_encoder("h265"), "hevc_vaapi");
}

#[test]
fn test_vaapi_quality_tables() {
    assert_eq!(vaapi_global_quality("h265", "veryhigh"), 22);
    assert_eq!(vaapi_global_quality("h265", "superlow"), 37);
    assert_eq!(vaapi_global_quality("h264", "good"), 24);
    assert_eq!(vaapi_global_quality("h264", "unknown"), 24);
}

#[test]
fn test_intel_quality_tables() {
    assert_eq!(intel_global_quality("h265", "veryhigh"), 18);
    assert_eq!(intel_global_quality("h264", "low"), 27);
    assert_eq!(intel_global_quality("h264", "unknown"), 24);
}

#[test]
fn test_prores_profile_mapping() {
    assert_eq!(prores_profile_num("superlow"), 0);
    assert_eq!(prores_profile_num("low"), 1);
    assert_eq!(prores_profile_num("medium"), 2);
    assert_eq!(prores_profile_num("high"), 3);
    assert_eq!(prores_profile_num("veryhigh"), 4);
}

fn quality_args(f: impl FnOnce(&mut Command)) -> Vec<String> {
    let mut cmd = Command::new("ffmpeg");
    f(&mut cmd);
    args(&cmd)
}

#[test]
fn nvidia_quality_args_add_h265_specific_aq() {
    // Common NVENC flags always present; h265 additionally enables spatial-aq.
    let h265 = quality_args(|c| FfmpegJob::add_nvidia_quality_args(c, "h265", "high", "medium"));
    assert!(h265.contains(&"-rc:v".into()), "{h265:?}");
    assert!(h265.contains(&"-spatial-aq".into()), "{h265:?}");
    let h264 = quality_args(|c| FfmpegJob::add_nvidia_quality_args(c, "h264", "high", "medium"));
    assert!(h264.contains(&"-rc:v".into()));
    assert!(
        !h264.contains(&"-spatial-aq".into()),
        "h264 must not set spatial-aq"
    );
}

#[test]
fn intel_quality_args_add_h264_profile() {
    let h264 = quality_args(|c| FfmpegJob::add_intel_quality_args(c, "h264", "high", "medium"));
    assert!(h264.contains(&"-global_quality".into()), "{h264:?}");
    assert!(h264.contains(&"high".into()), "h264 profile high: {h264:?}");
    let h265 = quality_args(|c| FfmpegJob::add_intel_quality_args(c, "h265", "high", "medium"));
    assert!(
        !h265.contains(&"-level:v".into()),
        "h265 must not add the h264 profile"
    );
}

#[test]
fn vaapi_quality_args_add_h264_profile() {
    let h264 = quality_args(|c| FfmpegJob::add_vaapi_quality_args(c, "h264", "high"));
    assert!(h264.contains(&"-rc_mode".into()), "{h264:?}");
    assert!(h264.contains(&"-level:v".into()), "h264 profile: {h264:?}");
    let h265 = quality_args(|c| FfmpegJob::add_vaapi_quality_args(c, "h265", "high"));
    assert!(
        !h265.contains(&"-level:v".into()),
        "h265 omits the h264 profile"
    );
}

#[test]
fn software_quality_args_route_per_codec() {
    let h264 = quality_args(|c| FfmpegJob::add_software_quality_args(c, "h264", "high", "medium"));
    assert!(h264.contains(&"-level:v".into()), "h264: {h264:?}");
    let h265 = quality_args(|c| FfmpegJob::add_software_quality_args(c, "h265", "high", "medium"));
    assert!(h265.contains(&"-x265-params".into()), "h265: {h265:?}");
    let av1 = quality_args(|c| FfmpegJob::add_software_quality_args(c, "av1", "high", "medium"));
    assert!(av1.contains(&"-svtav1-params".into()), "av1: {av1:?}");
    let vp9 = quality_args(|c| FfmpegJob::add_software_quality_args(c, "vp9", "high", "medium"));
    assert!(vp9.contains(&"-row-mt".into()), "vp9: {vp9:?}");
}
