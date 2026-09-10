use super::*;

#[test]
fn cpu_video_filters_default_empty() {
    let s = make_session(TranscodeFilters::default());
    assert!(s.build_cpu_video_filters().is_empty());
}

#[test]
fn cpu_video_filters_brightness() {
    let mut f = TranscodeFilters::default();
    f.brightness = 0.3;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    assert!(parts.iter().any(|p| p.contains("eq=brightness=0.30")));
}

#[test]
fn cpu_video_filters_contrast_and_saturation() {
    let mut f = TranscodeFilters::default();
    f.contrast = 1.5;
    f.saturation = 0.8;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    let eq_part = parts.iter().find(|p| p.starts_with("eq=")).unwrap();
    assert!(eq_part.contains("contrast=1.50"));
    assert!(eq_part.contains("saturation=0.80"));
}

#[test]
fn cpu_video_filters_sharpness() {
    let mut f = TranscodeFilters::default();
    f.sharpness = 1.0;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    assert!(parts.iter().any(|p| p == "unsharp=5:5:1.50:5:5:1.50"));
}

#[test]
fn cpu_video_filters_rotation_90() {
    let mut f = TranscodeFilters::default();
    f.rotation = 90;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    assert!(parts.contains(&"transpose=1".to_string()));
}

#[test]
fn cpu_video_filters_rotation_180() {
    let mut f = TranscodeFilters::default();
    f.rotation = 180;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    assert!(parts.contains(&"transpose=1,transpose=1".to_string()));
}

#[test]
fn cpu_video_filters_rotation_270() {
    let mut f = TranscodeFilters::default();
    f.rotation = 270;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    assert!(parts.contains(&"transpose=2".to_string()));
}

#[test]
fn cpu_video_filters_hflip() {
    let mut f = TranscodeFilters::default();
    f.hflip = true;
    let s = make_session(f);
    assert!(s.build_cpu_video_filters().contains(&"hflip".to_string()));
}

#[test]
fn cpu_video_filters_vflip() {
    let mut f = TranscodeFilters::default();
    f.vflip = true;
    let s = make_session(f);
    assert!(s.build_cpu_video_filters().contains(&"vflip".to_string()));
}

#[test]
fn cpu_video_filters_speed_setpts() {
    let mut f = TranscodeFilters::default();
    f.speed = 1.5;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    assert!(parts.iter().any(|p| p.starts_with("setpts=PTS/")));
}

#[test]
fn cpu_video_filters_text_subtitle_uses_input_path() {
    let mut f = TranscodeFilters::default();
    f.subtitle_track = Some(0);
    f.subtitle_is_bitmap = false;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    assert!(parts.iter().any(|p| p.contains("subtitles=")));
    assert!(parts.iter().any(|p| p.contains("si=0")));
}

#[test]
fn cpu_video_filters_text_subtitle_uses_extracted_path() {
    let mut f = TranscodeFilters::default();
    f.subtitle_track = Some(1);
    f.subtitle_is_bitmap = false;
    let mut s = make_session(f);
    s.extracted_sub_path = Some("/tmp/cast-sub-99-1.mkv".to_string());
    let parts = s.build_cpu_video_filters();
    assert!(parts.iter().any(|p| p.contains("cast-sub-99-1")));
    assert!(!parts.iter().any(|p| p.contains("si=")));
}

#[test]
fn cpu_video_filters_bitmap_subtitle_not_in_vf() {
    let mut f = TranscodeFilters::default();
    f.subtitle_track = Some(0);
    f.subtitle_is_bitmap = true;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    assert!(!parts.iter().any(|p| p.contains("subtitles=")));
}

#[test]
fn cpu_video_filters_combined_count() {
    let mut f = TranscodeFilters::default();
    f.brightness = 0.1;
    f.sharpness = 0.5;
    f.rotation = 90;
    f.hflip = true;
    f.speed = 1.5;
    let s = make_session(f);
    let parts = s.build_cpu_video_filters();
    assert_eq!(parts.len(), 5);
}

#[test]
fn stage1_fullgpu_nvidia_format_only() {
    let s = make_session(TranscodeFilters::default());
    let result = s.build_video_filters_for_stage(TranscodeStage::FullGpu, GpuType::Nvidia);
    assert_eq!(result, "scale_cuda=format=nv12");
}

#[test]
fn stage1_fullgpu_amd_format_only() {
    let s = make_session(TranscodeFilters::default());
    let result = s.build_video_filters_for_stage(TranscodeStage::FullGpu, GpuType::Amd);
    assert_eq!(result, "scale_vaapi=format=nv12");
}

#[test]
fn stage1_fullgpu_intel_format_only() {
    let s = make_session(TranscodeFilters::default());
    let result = s.build_video_filters_for_stage(TranscodeStage::FullGpu, GpuType::Intel);
    assert_eq!(result, "scale_qsv=format=nv12");
}

#[test]
fn stage2_partialgpu_nvidia_uploads_to_cuda() {
    let mut f = TranscodeFilters::default();
    f.brightness = 0.2;
    let s = make_session(f);
    let result = s.build_video_filters_for_stage(TranscodeStage::PartialGpu, GpuType::Nvidia);
    assert!(result.contains("eq="));
    assert!(result.contains("format=nv12"));
    assert!(result.contains("hwupload_cuda"));
}

#[test]
fn stage2_partialgpu_amd_uploads_to_vaapi() {
    let mut f = TranscodeFilters::default();
    f.brightness = 0.2;
    let s = make_session(f);
    let result = s.build_video_filters_for_stage(TranscodeStage::PartialGpu, GpuType::Amd);
    assert!(result.contains("hwupload=derive_device=vaapi"));
}

#[test]
fn stage3_software_adds_yuv420p() {
    let mut f = TranscodeFilters::default();
    f.brightness = 0.2;
    let s = make_session(f);
    let result = s.build_video_filters_for_stage(TranscodeStage::Software, GpuType::Cpu);
    assert!(result.contains("eq="));
    assert!(result.contains("format=yuv420p"));
    assert!(!result.contains("hwupload"));
}
