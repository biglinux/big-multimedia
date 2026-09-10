use super::*;

#[test]
fn ffmpeg_args_software_no_filters_has_basics() {
    let s = make_session(TranscodeFilters::default());
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    assert!(args.contains(&"-hide_banner".to_string()));
    assert!(args.contains(&"-i".to_string()));
    assert!(args.contains(&"-c:v".to_string()));
    assert!(args.contains(&"libx264".to_string()));
    assert!(args.contains(&"-c:a".to_string()));
    assert!(args.contains(&"aac".to_string()));
    assert!(args.contains(&"-f".to_string()));
    assert!(args.contains(&"mp4".to_string()));
    assert!(args.contains(&"pipe:1".to_string()));
}

#[test]
fn ffmpeg_args_seek_adds_ss_before_input() {
    let s = make_session(TranscodeFilters::default());
    let args = s.build_ffmpeg_args(30.0, TranscodeStage::Software, GpuType::Cpu, "");
    let ss_pos = args.iter().position(|a| a == "-ss").unwrap();
    let i_pos = args.iter().position(|a| a == "-i").unwrap();
    assert!(ss_pos < i_pos, "-ss must precede -i for fast seeking");
    assert_eq!(args[ss_pos + 1], "30.000");
}

#[test]
fn ffmpeg_args_no_seek_when_zero() {
    let s = make_session(TranscodeFilters::default());
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    assert!(!args.contains(&"-ss".to_string()));
}

#[test]
fn ffmpeg_args_nvidia_fullgpu_encoder() {
    let s = make_session(TranscodeFilters::default());
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::FullGpu, GpuType::Nvidia, "");
    assert!(args.contains(&"h264_nvenc".to_string()));
}

#[test]
fn ffmpeg_args_amd_fullgpu_encoder() {
    let s = make_session(TranscodeFilters::default());
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::FullGpu, GpuType::Amd, "");
    assert!(args.contains(&"h264_vaapi".to_string()));
}

#[test]
fn ffmpeg_args_intel_fullgpu_encoder() {
    let s = make_session(TranscodeFilters::default());
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::FullGpu, GpuType::Intel, "");
    assert!(args.contains(&"h264_qsv".to_string()));
}

#[test]
fn ffmpeg_args_audio_track_selection() {
    let mut f = TranscodeFilters::default();
    f.audio_track = Some(2);
    let s = make_session(f);
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    assert!(args.contains(&"0:a:2".to_string()));
}

#[test]
fn ffmpeg_args_default_audio_track_zero() {
    let s = make_session(TranscodeFilters::default());
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    assert!(args.contains(&"0:a:0".to_string()));
}

#[test]
fn ffmpeg_args_volume_produces_af_flag() {
    let mut f = TranscodeFilters::default();
    f.volume = 0.5;
    let s = make_session(f);
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    assert!(args.contains(&"-af".to_string()));
}

#[test]
fn ffmpeg_args_fragmented_mp4_output() {
    let s = make_session(TranscodeFilters::default());
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    let movflags_pos = args.iter().position(|a| a == "-movflags").unwrap();
    let flags = &args[movflags_pos + 1];
    assert!(flags.contains("frag_keyframe"));
    assert!(flags.contains("empty_moov"));
    assert!(flags.contains("default_base_moof"));
}

#[test]
fn ffmpeg_args_text_subtitle_uses_vf_not_filter_complex() {
    let mut f = TranscodeFilters::default();
    f.subtitle_track = Some(0);
    f.subtitle_is_bitmap = false;
    let s = make_session(f);
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    assert!(args.contains(&"-vf".to_string()));
    assert!(!args.contains(&"-filter_complex".to_string()));
}

#[test]
fn ffmpeg_args_bitmap_subtitle_uses_filter_complex() {
    let mut f = TranscodeFilters::default();
    f.subtitle_track = Some(0);
    f.subtitle_is_bitmap = true;
    let s = make_session(f);
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    assert!(args.contains(&"-filter_complex".to_string()));
    assert!(args.contains(&"[v]".to_string()));
}

#[test]
fn ffmpeg_args_bitmap_subtitle_maps_output_label() {
    let mut f = TranscodeFilters::default();
    f.subtitle_track = Some(0);
    f.subtitle_is_bitmap = true;
    let s = make_session(f);
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    let map_positions: Vec<usize> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| a.as_str() == "-map")
        .map(|(i, _)| i)
        .collect();
    assert!(
        map_positions.iter().any(|&i| args[i + 1] == "[v]"),
        "bitmap subs require -map [v]"
    );
}

#[test]
fn ffmpeg_args_aac_stereo_192k() {
    let s = make_session(TranscodeFilters::default());
    let args = s.build_ffmpeg_args(0.0, TranscodeStage::Software, GpuType::Cpu, "");
    assert!(args.contains(&"192k".to_string()));
    assert!(args.contains(&"2".to_string()));
}
