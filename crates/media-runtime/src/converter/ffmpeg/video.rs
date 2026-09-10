//! Video encoder selection + filter chain build. Encoder picked from
//! (codec, GpuType, GpuStage); filter chain wraps software filters in
//! hwdownload/hwupload around GPU paths because VAAPI/QSV/CUDA can't run
//! `eq`, `crop`, `transpose`, etc. on hardware surfaces directly.

use super::*;

impl FfmpegJob {
    pub(crate) fn add_video_args(&self, cmd: &mut Command) {
        let codec = &self.profile.video.codec;

        if codec == "copy" {
            cmd.args(["-c:v", "copy"]);
            return;
        }

        let encoder = self.resolve_encoder(codec);
        cmd.args(["-c:v", encoder]);

        let quality = &self.profile.video.quality;
        let preset = &self.profile.video.preset;

        match self.gpu.gpu_type {
            GpuType::Nvidia => Self::add_nvidia_quality_args(cmd, codec, quality, preset),
            GpuType::Intel => Self::add_intel_quality_args(cmd, codec, quality, preset),
            GpuType::Amd => Self::add_vaapi_quality_args(cmd, codec, quality),
            GpuType::Cpu => Self::add_software_quality_args(cmd, codec, quality, preset),
        }

        if self.stage == GpuStage::Software && codec == "av1" {
            cmd.args(["-pix_fmt", "yuv420p10le"]);
        } else if self.profile.video.pixel_format != "auto" {
            cmd.args(["-pix_fmt", &self.profile.video.pixel_format]);
        }
    }

    pub(crate) fn resolve_encoder(&self, codec: &str) -> &'static str {
        if self.stage == GpuStage::Software {
            return match codec {
                "h265" => "libx265",
                "av1" => "libsvtav1",
                "vp9" => "libvpx-vp9",
                "prores" => "prores_ks",
                _ => "libx264",
            };
        }

        match codec {
            "h264" => match self.gpu.gpu_type {
                GpuType::Nvidia => "h264_nvenc",
                GpuType::Amd => "h264_vaapi",
                GpuType::Intel => "h264_qsv",
                GpuType::Cpu => "libx264",
            },
            "h265" => match self.gpu.gpu_type {
                GpuType::Nvidia => "hevc_nvenc",
                GpuType::Amd => "hevc_vaapi",
                GpuType::Intel => "hevc_qsv",
                GpuType::Cpu => "libx265",
            },
            "av1" => match self.gpu.gpu_type {
                GpuType::Nvidia => "av1_nvenc",
                GpuType::Amd => "av1_vaapi",
                GpuType::Intel => "av1_qsv",
                GpuType::Cpu => "libsvtav1",
            },
            "vp9" => match self.gpu.gpu_type {
                GpuType::Intel => "vp9_qsv",
                GpuType::Nvidia | GpuType::Amd => "vp9_vaapi",
                GpuType::Cpu => "libvpx-vp9",
            },
            "prores" => "prores_ks",
            _ => "libx264",
        }
    }

    pub fn build_video_filters(&self) -> String {
        if self.skip_video_filters {
            return String::new();
        }

        let vf = &self.profile.video.filters;
        let mut sw_filters = Vec::new();

        let (brightness, contrast, saturation, hue, rotation, flip_h, flip_v) =
            if let Some(ref ov) = self.video_overrides {
                (
                    ov.brightness,
                    ov.contrast,
                    ov.saturation,
                    ov.hue,
                    ov.rotation,
                    ov.flip_h,
                    ov.flip_v,
                )
            } else {
                (
                    vf.brightness,
                    vf.contrast,
                    vf.saturation,
                    vf.hue,
                    vf.rotation,
                    vf.flip_h,
                    vf.flip_v,
                )
            };

        if let Some(ref crop) = self.crop {
            sw_filters.push(format!("crop={}:{}:{}:{}", crop.w, crop.h, crop.x, crop.y));
        } else if let Some(ref crop) = vf.crop {
            sw_filters.push(format!("crop={}:{}:{}:{}", crop.w, crop.h, crop.x, crop.y));
        }

        match rotation {
            90 => sw_filters.push("transpose=1".into()),
            180 => sw_filters.push("transpose=1,transpose=1".into()),
            270 => sw_filters.push("transpose=2".into()),
            _ => {}
        }

        if flip_h {
            sw_filters.push("hflip".into());
        }
        if flip_v {
            sw_filters.push("vflip".into());
        }

        let runtime_config = crate::converter::runtime_config::config();
        let contrast_ffmpeg = runtime_config.contrast.map_preview_to_ffmpeg(contrast);
        let sat_ffmpeg = runtime_config.saturation.map_preview_to_ffmpeg(saturation);
        let has_color = brightness.abs() > runtime_config.float_threshold
            || (contrast_ffmpeg - runtime_config.contrast.neutral).abs()
                > runtime_config.float_threshold
            || (sat_ffmpeg - runtime_config.saturation.neutral).abs()
                > runtime_config.float_threshold
            || hue.abs() > runtime_config.float_threshold;
        if has_color {
            sw_filters.push(format!(
                "eq=brightness={brightness}:contrast={contrast_ffmpeg}:saturation={sat_ffmpeg}:hue={hue}"
            ));
        }

        if let SubtitleInput::ExternalFile(ref path) = self.subtitle {
            let escaped = path.to_string_lossy().replace(':', "\\:");
            sw_filters.push(format!("subtitles='{escaped}'"));
        }

        let has_scale = vf.resolution != "auto" && !vf.resolution.is_empty();
        if sw_filters.is_empty() && !has_scale {
            return String::new();
        }

        match self.stage {
            GpuStage::FullGpu => {
                let mut chain = Vec::new();

                if !sw_filters.is_empty() {
                    match self.gpu.gpu_type {
                        GpuType::Intel => {
                            chain.push("hwdownload".into());
                            chain.push("format=nv12".into());
                            chain.extend(sw_filters);
                            chain.push("hwupload=extra_hw_frames=64,format=qsv".into());
                        }
                        GpuType::Nvidia => {
                            chain.push("hwdownload".into());
                            chain.push("format=nv12".into());
                            chain.extend(sw_filters);
                            chain.push("hwupload_cuda".into());
                        }
                        _ => {
                            chain.push("hwdownload".into());
                            chain.push("format=nv12".into());
                            chain.extend(sw_filters);
                            chain.push("hwupload".into());
                        }
                    }
                }

                if has_scale && let Some((w, h)) = vf.resolution.split_once('x') {
                    match self.gpu.gpu_type {
                        GpuType::Nvidia => chain.push(format!("scale_cuda={w}:{h}")),
                        GpuType::Intel => chain.push(format!("scale_qsv=w={w}:h={h}")),
                        _ => chain.push(format!("scale_vaapi=w={w}:h={h}")),
                    }
                }

                chain.join(",")
            }
            GpuStage::PartialGpu => {
                let mut chain = Vec::new();
                chain.extend(sw_filters);

                match self.gpu.gpu_type {
                    GpuType::Intel => {
                        chain.push("format=nv12".into());
                        chain.push("hwupload=extra_hw_frames=64,format=qsv".into());
                    }
                    GpuType::Nvidia => {
                        chain.push("format=nv12".into());
                        chain.push("hwupload_cuda".into());
                    }
                    _ => {
                        chain.push("format=nv12".into());
                        chain.push("hwupload".into());
                    }
                }

                if has_scale && let Some((w, h)) = vf.resolution.split_once('x') {
                    match self.gpu.gpu_type {
                        GpuType::Nvidia => chain.push(format!("scale_cuda={w}:{h}")),
                        GpuType::Intel => chain.push(format!("scale_qsv=w={w}:h={h}")),
                        _ => chain.push(format!("scale_vaapi=w={w}:h={h}")),
                    }
                }

                chain.join(",")
            }
            GpuStage::Software => {
                let mut chain = sw_filters;
                if has_scale && let Some((w, h)) = vf.resolution.split_once('x') {
                    chain.push(format!("scale={w}:{h}"));
                }
                chain.join(",")
            }
        }
    }
}
