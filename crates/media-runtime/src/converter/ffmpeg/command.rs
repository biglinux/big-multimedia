//! Top-level ffmpeg command assembly: hwaccel init, input + trim, stream
//! mapping (with bitmap-sub filtering for MP4/MOV containers), per-codec
//! audio/video/subtitle args, +faststart for MP4-family, and `-progress
//! pipe:2` for the live progress parser.

use super::*;

impl FfmpegJob {
    // bigagents: app-local-subprocess — assembles the ffmpeg argv across many
    // `add_*_args(&mut Command)` helpers (hwaccel, decode, map, codec, faststart)
    // and emits `-progress pipe:2` for the live progress parser; the returned
    // Command is spawned with streaming stderr. BigSubprocessSpec models
    // run/spawn but not this `&mut Command` builder threading + progress-pipe
    // contract, so the converter core stays on std::process::Command by design.
    pub fn build_command(&self) -> Command {
        let mut cmd = Command::new("ffmpeg");
        cmd.stdin(Stdio::null());

        cmd.args(["-nostdin", "-hide_banner", "-nostats", "-y"]);

        if self.stage == GpuStage::FullGpu {
            self.add_hw_init_args(&mut cmd);
            self.add_hw_decode_args(&mut cmd);
        } else if self.stage == GpuStage::PartialGpu {
            self.add_hw_init_args(&mut cmd);
        }

        if let Some(start) = self.trim.start {
            cmd.args(["-ss", &ffmpeg_timestamp(start)]);
        }
        cmd.arg("-i").arg(&self.input);

        if let Some(end) = self.trim.end {
            let duration = end - self.trim.start.unwrap_or(0.0);
            if duration > 0.0 {
                cmd.args(["-t", &ffmpeg_timestamp(duration)]);
            }
        }

        if let SubtitleInput::ExternalFile(ref path) = self.subtitle {
            cmd.arg("-i").arg(path);
        }

        if self.audio_only {
            cmd.arg("-vn");
        } else {
            let vtrack = self.video_track.unwrap_or(0);
            cmd.args(["-map", &format!("0:v:{vtrack}")]);
        }

        let (afilters, use_filter_complex) = self.build_audio_filters();
        if !use_filter_complex {
            let atrack = self.audio_track.unwrap_or(0);
            cmd.args(["-map", &format!("0:a:{atrack}?")]);
        }

        if !self.audio_only && self.profile.subtitles.handling == "embedded" {
            let container = profiles::resolve_container(&self.profile);
            let needs_filter = matches!(container, "mp4" | "mov" | "m4v");

            if matches!(self.subtitle, SubtitleInput::ExternalFile(_)) {
                cmd.args(["-map", "1:s:0"]);
            } else if needs_filter && !self.subtitle_streams.is_empty() {
                for ss in &self.subtitle_streams {
                    if !ss.is_bitmap {
                        cmd.args(["-map", &format!("0:{}", ss.index)]);
                    }
                }
            } else {
                cmd.args(["-map", "0:s?"]);
            }
        }

        if !self.audio_only {
            self.add_video_args(&mut cmd);

            let vfilters = self.build_video_filters();
            if !vfilters.is_empty() {
                cmd.args(["-vf", &vfilters]);
            }
        }

        self.add_audio_args(&mut cmd);

        if !afilters.is_empty() {
            if use_filter_complex {
                cmd.args(["-filter_complex", &afilters, "-map", "[aout]"]);
            } else {
                cmd.args(["-af", &afilters]);
            }
        }

        if !self.audio_only {
            self.add_subtitle_args(&mut cmd);
        }

        let container = profiles::resolve_container(&self.profile);
        if matches!(container, "mp4" | "mov" | "m4v") {
            cmd.args(["-movflags", "+faststart"]);
        }

        cmd.args(["-progress", "pipe:2", "-stats_period", "0.5"]);
        // `--` end-of-options: output is positional; defends against
        // user-controlled paths beginning with `-`.
        cmd.arg("--").arg(&self.output);

        info!("FFmpeg command prepared (media paths omitted)");
        cmd
    }

    fn add_hw_init_args(&self, cmd: &mut Command) {
        let device = if self.gpu.device_path.is_empty() {
            "/dev/dri/renderD128"
        } else {
            &self.gpu.device_path
        };

        match self.gpu.gpu_type {
            GpuType::Nvidia => {
                if self.gpu.device_path.is_empty() {
                    cmd.args(["-init_hw_device", "cuda=cu"]);
                } else {
                    cmd.args(["-init_hw_device", &format!("cuda=cu:{device}")]);
                }
            }
            GpuType::Intel => {
                cmd.args([
                    "-init_hw_device",
                    "vaapi=va:,driver=iHD,kernel_driver=i915",
                    "-init_hw_device",
                    "qsv=qs@va",
                ]);
            }
            GpuType::Amd => {
                cmd.args(["-init_hw_device", &format!("vaapi=va:{device}")]);
            }
            GpuType::Cpu => {}
        }
    }

    // FullGpu only — PartialGpu skips hwaccel decode.
    fn add_hw_decode_args(&self, cmd: &mut Command) {
        match self.gpu.gpu_type {
            GpuType::Nvidia => {
                cmd.args(["-hwaccel", "cuda", "-hwaccel_output_format", "cuda"]);
            }
            GpuType::Intel => {
                cmd.args([
                    "-filter_hw_device",
                    "qs",
                    "-hwaccel",
                    "qsv",
                    "-hwaccel_output_format",
                    "qsv",
                ]);
            }
            GpuType::Amd => {
                cmd.args(["-hwaccel", "vaapi", "-hwaccel_output_format", "vaapi"]);
            }
            GpuType::Cpu => {}
        }
    }

    fn add_audio_args(&self, cmd: &mut Command) {
        let audio = &self.profile.audio;

        match audio.handling.as_str() {
            "copy" => {
                cmd.args(["-c:a", "copy"]);
            }
            "none" => {
                cmd.arg("-an");
            }
            _ => {
                let acodec = match audio.codec.as_str() {
                    "aac" => "aac",
                    "opus" => "libopus",
                    "ac3" => "ac3",
                    "mp3" => "libmp3lame",
                    "flac" => "flac",
                    "vorbis" => "libvorbis",
                    "pcm" => "pcm_s16le",
                    other => other,
                };
                cmd.args(["-c:a", acodec]);

                if acodec == "aac" {
                    cmd.args(["-aac_coder", "fast"]);
                    cmd.args(["-profile:a", "aac_low"]);
                }

                if !matches!(audio.codec.as_str(), "flac" | "pcm") {
                    cmd.args(["-b:a", &audio.bitrate]);
                }
            }
        }
    }

    fn add_subtitle_args(&self, cmd: &mut Command) {
        let subs = &self.profile.subtitles;
        match subs.handling.as_str() {
            "burn" | "none" | "extract" => {
                cmd.arg("-sn");
            }
            _ => {
                let container = profiles::resolve_container(&self.profile);
                match container {
                    "mp4" | "mov" | "m4v" => {
                        cmd.args(["-c:s", "mov_text"]);
                    }
                    _ => {
                        cmd.args(["-c:s", "copy"]);
                    }
                }
            }
        }
    }
}
