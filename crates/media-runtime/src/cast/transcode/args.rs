use crate::cast::gpu::GpuType;

use super::{CAST_HIGHPASS_HZ, TranscodeSession, TranscodeStage};

/// Subtitle handling flags resolved from the requested filters + seek offset.
struct SubFlags {
    /// Text subtitle track (SRT/ASS) being burned in via `-vf subtitles=`.
    use_text: bool,
    /// Bitmap subtitle track (PGS/DVBSUB) being burned in via `filter_complex`.
    use_bitmap: bool,
    /// Whether `-copyts` is needed to keep subtitle timestamps aligned after seek.
    needs_copyts: bool,
}

impl SubFlags {
    fn from_session(session: &TranscodeSession, seek_secs: f64) -> Self {
        let has_track = session.filters.subtitle_track.is_some();
        let is_bitmap = session.filters.subtitle_is_bitmap;
        let use_text = has_track && !is_bitmap;
        let use_bitmap = has_track && is_bitmap;
        Self {
            use_text,
            use_bitmap,
            needs_copyts: (use_text || use_bitmap) && seek_secs > 0.5,
        }
    }
}

/// Append `filter` to an existing chain (`","` delimited); initialise when
/// empty. Used to fold in `setpts=PTS-STARTPTS` / `asetpts=PTS-STARTPTS`.
fn join_filter(chain: &mut String, filter: &str) {
    if chain.is_empty() {
        chain.push_str(filter);
    } else {
        chain.push(',');
        chain.push_str(filter);
    }
}

/// Pre-`-i` arguments: seek, copyts, probe sizing for bitmap subs, hwaccel.
fn append_pre_input_args(
    args: &mut Vec<String>,
    seek_secs: f64,
    subs: &SubFlags,
    stage: TranscodeStage,
    gpu: GpuType,
    device_path: &str,
) {
    if seek_secs > 0.5 {
        args.extend(["-ss".into(), format!("{seek_secs:.3}")]);
    }
    if subs.needs_copyts {
        args.push("-copyts".into());
    }
    if subs.use_bitmap {
        // Bitmap subs need deep probing to locate the subtitle stream headers.
        args.extend([
            "-analyzeduration".into(),
            "200000000".into(),
            "-probesize".into(),
            "200M".into(),
        ]);
    }
    append_hwaccel_args(args, stage, gpu, device_path);
}

fn append_hwaccel_args(
    args: &mut Vec<String>,
    stage: TranscodeStage,
    gpu: GpuType,
    device_path: &str,
) {
    match stage {
        TranscodeStage::FullGpu => {
            args.extend(gpu.init_hw_args(device_path));
            args.extend(gpu.hwaccel_decode_args().iter().map(|s| (*s).into()));
        }
        TranscodeStage::PartialGpu => {
            args.extend(gpu.init_hw_args(device_path));
            args.extend(gpu.partial_gpu_args().iter().map(|s| (*s).into()));
        }
        TranscodeStage::Software => {}
    }
}

/// Fragmented-MP4 muxer config for pipe streaming.
fn append_output_format(args: &mut Vec<String>) {
    args.extend([
        "-f".into(),
        "mp4".into(),
        "-movflags".into(),
        "frag_keyframe+empty_moov+default_base_moof".into(),
        "pipe:1".into(),
    ]);
}

impl TranscodeSession {
    pub(super) fn build_ffmpeg_args(
        &self,
        seek_secs: f64,
        stage: TranscodeStage,
        gpu: GpuType,
        device_path: &str,
    ) -> Vec<String> {
        let subs = SubFlags::from_session(self, seek_secs);
        let mut args: Vec<String> =
            vec!["-hide_banner".into(), "-loglevel".into(), "warning".into()];

        append_pre_input_args(&mut args, seek_secs, &subs, stage, gpu, device_path);
        args.extend(["-i".into(), self.input_path.clone()]);

        let audio_idx = self.filters.audio_track.unwrap_or(0);
        self.append_video_mapping(&mut args, &subs, audio_idx, stage, gpu);

        args.extend(["-c:v".into(), gpu.h264_encoder().into()]);
        args.extend(gpu.quality_args().iter().map(|s| (*s).into()));

        self.append_audio_filters(&mut args, &subs);
        append_output_format(&mut args);
        args
    }

    /// Choose between bitmap (`filter_complex` + overlay) and direct mapping.
    fn append_video_mapping(
        &self,
        args: &mut Vec<String>,
        subs: &SubFlags,
        audio_idx: usize,
        stage: TranscodeStage,
        gpu: GpuType,
    ) {
        if subs.use_bitmap {
            self.append_bitmap_subtitle_mapping(args, subs, audio_idx, stage, gpu);
        } else {
            self.append_direct_mapping(args, subs, audio_idx, stage, gpu);
        }
    }

    fn append_bitmap_subtitle_mapping(
        &self,
        args: &mut Vec<String>,
        subs: &SubFlags,
        audio_idx: usize,
        stage: TranscodeStage,
        gpu: GpuType,
    ) {
        let sub_idx = self
            .filters
            .subtitle_track
            .expect("bitmap sub path requires subtitle_track");
        let vf = self.build_video_filters_for_stage(stage, gpu);
        let mut vf_chain = if vf.is_empty() {
            String::new()
        } else {
            format!(",{vf}")
        };
        if subs.needs_copyts {
            vf_chain.push_str(",setpts=PTS-STARTPTS");
        }
        let filter_complex = if self.video_width > 0 && self.video_height > 0 {
            format!(
                "[0:s:{}]scale={}:{}[sub];[0:v][sub]overlay=eof_action=pass{}[v]",
                sub_idx, self.video_width, self.video_height, vf_chain
            )
        } else {
            format!("[0:v][0:s:{sub_idx}]overlay=eof_action=pass{vf_chain}[v]")
        };
        args.extend([
            "-filter_complex".into(),
            filter_complex,
            "-map".into(),
            "[v]".into(),
            "-map".into(),
            format!("0:a:{audio_idx}"),
        ]);
    }

    fn append_direct_mapping(
        &self,
        args: &mut Vec<String>,
        subs: &SubFlags,
        audio_idx: usize,
        stage: TranscodeStage,
        gpu: GpuType,
    ) {
        args.extend(["-map".into(), "0:v:0".into()]);
        args.extend(["-map".into(), format!("0:a:{audio_idx}")]);

        let mut vf = self.build_video_filters_for_stage(stage, gpu);
        if subs.use_text && subs.needs_copyts {
            join_filter(&mut vf, "setpts=PTS-STARTPTS");
        }
        if !vf.is_empty() {
            args.extend(["-vf".into(), vf]);
        }
    }

    fn append_audio_filters(&self, args: &mut Vec<String>, subs: &SubFlags) {
        let mut af = self.build_audio_filters();
        if subs.needs_copyts {
            join_filter(&mut af, "asetpts=PTS-STARTPTS");
        }
        if !af.is_empty() {
            args.extend(["-af".into(), af]);
        }
        args.extend([
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "192k".into(),
            "-ac".into(),
            "2".into(),
        ]);
    }

    // common CPU-side video filters → subtitles, eq, sharpness, rotation, flip, speed
    pub(super) fn build_cpu_video_filters(&self) -> Vec<String> {
        let f = &self.filters;
        let mut parts = Vec::new();

        if let Some(sub_idx) = f.subtitle_track
            && !f.subtitle_is_bitmap
        {
            if let Some(ref extracted) = self.extracted_sub_path {
                let escaped = extracted
                    .replace('\\', "\\\\")
                    .replace(':', "\\:")
                    .replace('\'', "\\'");
                parts.push(format!("subtitles='{escaped}'"));
            } else {
                let escaped = self
                    .input_path
                    .replace('\\', "\\\\")
                    .replace(':', "\\:")
                    .replace('\'', "\\'");
                parts.push(format!("subtitles='{escaped}'\\:si={sub_idx}"));
            }
        }

        if f.brightness.abs() > f32::EPSILON
            || (f.contrast - 1.0).abs() > f32::EPSILON
            || (f.saturation - 1.0).abs() > f32::EPSILON
        {
            parts.push(format!(
                "eq=brightness={:.2}:contrast={:.2}:saturation={:.2}",
                f.brightness, f.contrast, f.saturation
            ));
        }

        if f.sharpness > 0.0 {
            let amount = f.sharpness * 1.5;
            parts.push(format!("unsharp=5:5:{amount:.2}:5:5:{amount:.2}"));
        }

        match f.rotation {
            90 => parts.push("transpose=1".into()),
            180 => parts.push("transpose=1,transpose=1".into()),
            270 => parts.push("transpose=2".into()),
            _ => {}
        }

        if f.hflip {
            parts.push("hflip".into());
        }
        if f.vflip {
            parts.push("vflip".into());
        }

        if (f.speed - 1.0).abs() > 0.01 {
            parts.push(format!("setpts=PTS/{:.4}", f.speed));
        }

        parts
    }

    /// Build video filter chain, adapted per transcode stage.
    ///
    /// - Stage 1 (FullGpu): Only format conversion (scale_cuda/scale_vaapi/scale_qsv=format=nv12).
    ///   Video filters are not used in Stage 1 (pipeline skips to Stage 2 if filters needed).
    /// - Stage 2 (PartialGpu): CPU-side filters + upload to GPU for encoding.
    /// - Stage 3 (Software): CPU-side filters + format=yuv420p.
    pub(super) fn build_video_filters_for_stage(
        &self,
        stage: TranscodeStage,
        gpu: GpuType,
    ) -> String {
        match stage {
            TranscodeStage::FullGpu => gpu.stage1_format_filter().to_string(),
            TranscodeStage::PartialGpu => {
                let mut parts = self.build_cpu_video_filters();
                for upload_filter in gpu.partial_upload_filters() {
                    parts.push(upload_filter.to_string());
                }
                parts.join(",")
            }
            TranscodeStage::Software => {
                let mut parts = self.build_cpu_video_filters();
                parts.push("format=yuv420p".into());
                parts.join(",")
            }
        }
    }

    pub(super) fn build_audio_filters(&self) -> String {
        let f = &self.filters;
        let mut parts = Vec::new();

        if f.has_audio_filters() {
            parts.push(format!("highpass=f={CAST_HIGHPASS_HZ}:poles=2"));
        }

        if f.voice_enhance && big_audio_effects::noise_reduction::gtcrn_available() {
            parts.push(
                big_audio_effects::noise_reduction::gtcrn_cast_ladspa_filter(f64::from(
                    f.noise_strength,
                )),
            );
        }

        if f.voice_enhance {
            parts.push("acompressor=threshold=-20dB:ratio=4:attack=5:release=250".into());
        }

        for (freq, gain) in &f.eq_bands {
            if *gain != 0.0 {
                parts.push(format!(
                    "equalizer=f={freq:.0}:width_type=o:width=1:g={gain:.1}"
                ));
            }
        }

        if (f.volume - 1.0).abs() > 0.01 {
            parts.push(format!("volume={:.4}", f.volume));
        }

        if (f.speed - 1.0).abs() > 0.01 {
            let mut remaining = f64::from(f.speed);
            while remaining > 100.0 {
                parts.push("atempo=100.0".into());
                remaining /= 100.0;
            }
            while remaining < 0.5 {
                parts.push("atempo=0.5".into());
                remaining /= 0.5;
            }
            parts.push(format!("atempo={remaining:.4}"));
        }

        parts.join(",")
    }
}
