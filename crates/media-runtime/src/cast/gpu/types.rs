/// GPU type for transcoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuType {
    Nvidia,
    Amd,
    Intel,
    Cpu,
}

/// A detected GPU and the identifier accepted by its FFmpeg backend.
#[derive(Debug, Clone)]
pub struct GpuDevice {
    /// GPU vendor type.
    pub gpu_type: GpuType,
    /// Human-readable name (e.g. "NVIDIA GeForce GTX 1660 Ti").
    pub name: String,
    /// CUDA ordinal for NVIDIA; DRM render node for VAAPI/QSV. Empty for CPU.
    pub device_path: String,
}

impl GpuDevice {
    /// User-friendly display label for UI dropdowns.
    pub fn label(&self) -> String {
        if self.gpu_type == GpuType::Cpu {
            return "CPU (Software)".to_string();
        }
        let model = self.friendly_model();
        let vendor = self.vendor_name();
        let model = model.strip_prefix(vendor).unwrap_or(&model).trim();
        if model.is_empty() {
            return vendor.to_string();
        }
        format!("{vendor} ({model})")
    }

    fn vendor_name(&self) -> &str {
        let low = self.name.to_lowercase();
        if low.contains("nvidia") {
            "NVIDIA"
        } else if low.contains("intel") {
            "Intel"
        } else if low.contains("amd") || low.contains("ati") || low.contains("advanced micro") {
            "AMD"
        } else {
            match self.gpu_type {
                GpuType::Nvidia => "NVIDIA",
                GpuType::Amd => "AMD",
                GpuType::Intel => "Intel",
                GpuType::Cpu => "CPU",
            }
        }
    }

    fn friendly_model(&self) -> String {
        if let Some(start) = self.name.find('[')
            && let Some(end) = self.name[start..].find(']')
        {
            let model = &self.name[start + 1..start + end];
            if !model.is_empty() {
                return model.to_string();
            }
        }

        let mut clean = self.name.clone();
        if let Some(pos) = clean.find("(rev") {
            clean = clean[..pos].trim().to_string();
        }
        clean = clean
            .replace("Corporation", "")
            .replace("Advanced Micro Devices, Inc.", "AMD")
            .replace("  ", " ");
        clean.trim().to_string()
    }
}

impl GpuType {
    /// FFmpeg backend for this codec/API pair. Driver and silicon support still
    /// need a functional probe; an available backend does not guarantee either.
    pub fn video_encoder(self, codec: &str) -> Option<&'static str> {
        match (self, codec) {
            (Self::Nvidia, "h264") => Some("h264_nvenc"),
            (Self::Nvidia, "h265") => Some("hevc_nvenc"),
            (Self::Nvidia, "av1") => Some("av1_nvenc"),
            (Self::Intel, "h264") => Some("h264_qsv"),
            (Self::Intel, "h265") => Some("hevc_qsv"),
            (Self::Intel, "av1") => Some("av1_qsv"),
            (Self::Intel, "vp9") => Some("vp9_qsv"),
            (Self::Amd, "h264") => Some("h264_vaapi"),
            (Self::Amd, "h265") => Some("hevc_vaapi"),
            (Self::Amd, "av1") => Some("av1_vaapi"),
            (Self::Amd, "vp9") => Some("vp9_vaapi"),
            _ => None,
        }
    }

    pub fn h264_encoder(self) -> &'static str {
        self.video_encoder("h264").unwrap_or("libx264")
    }

    pub fn init_hw_args(self, device_path: &str) -> Vec<String> {
        match self {
            GpuType::Nvidia => {
                let device = if device_path.is_empty() {
                    "0"
                } else {
                    device_path
                };
                vec!["-init_hw_device".into(), format!("cuda=cu:{device}")]
            }
            GpuType::Intel => vec![
                "-init_hw_device".into(),
                if device_path.is_empty() {
                    "vaapi=va:,driver=iHD,kernel_driver=i915".into()
                } else {
                    format!("vaapi=va:{device_path}")
                },
                "-init_hw_device".into(),
                "qsv=qs@va".into(),
            ],
            GpuType::Amd => {
                if device_path.is_empty() {
                    vec!["-init_hw_device".into(), "vaapi=va".into()]
                } else {
                    vec!["-init_hw_device".into(), format!("vaapi=va:{device_path}")]
                }
            }
            GpuType::Cpu => vec![],
        }
    }

    pub fn hwaccel_decode_args(self) -> Vec<&'static str> {
        match self {
            GpuType::Nvidia => vec![
                "-filter_hw_device",
                "cu",
                "-hwaccel",
                "cuda",
                "-hwaccel_device",
                "cu",
                "-hwaccel_output_format",
                "cuda",
            ],
            GpuType::Amd => vec![
                "-filter_hw_device",
                "va",
                "-hwaccel",
                "vaapi",
                "-hwaccel_device",
                "va",
                "-hwaccel_output_format",
                "vaapi",
            ],
            GpuType::Intel => vec![
                "-filter_hw_device",
                "qs",
                "-hwaccel",
                "qsv",
                "-hwaccel_device",
                "qs",
                "-hwaccel_output_format",
                "qsv",
            ],
            GpuType::Cpu => vec![],
        }
    }

    pub fn hwaccel_args(self) -> Vec<&'static str> {
        match self {
            GpuType::Nvidia => vec!["-hwaccel", "cuda"],
            GpuType::Amd => vec!["-hwaccel", "vaapi"],
            GpuType::Intel => vec!["-filter_hw_device", "qs", "-hwaccel", "qsv"],
            GpuType::Cpu => vec![],
        }
    }

    pub fn partial_gpu_args(self) -> Vec<&'static str> {
        match self {
            GpuType::Intel => vec!["-filter_hw_device", "qs"],
            GpuType::Nvidia => vec!["-filter_hw_device", "cu"],
            GpuType::Amd => vec!["-filter_hw_device", "va"],
            GpuType::Cpu => vec![],
        }
    }

    pub fn scale_filter(self) -> &'static str {
        match self {
            GpuType::Nvidia => "scale_cuda",
            GpuType::Amd => "scale_vaapi",
            GpuType::Intel => "scale_qsv",
            GpuType::Cpu => "scale",
        }
    }

    pub fn quality_args(self) -> Vec<&'static str> {
        match self {
            GpuType::Nvidia => vec![
                "-rc:v",
                "vbr",
                "-cq",
                "28",
                "-tune:v",
                "hq",
                "-preset:v",
                "p4",
                "-temporal-aq",
                "1",
                "-rc-lookahead",
                "100",
            ],
            GpuType::Amd => vec![
                "-profile:v",
                "high",
                "-level:v",
                "4.1",
                "-rc_mode",
                "CQP",
                "-global_quality",
                "24",
            ],
            GpuType::Intel => vec![
                "-preset",
                "medium",
                "-profile:v",
                "high",
                "-level:v",
                "4.1",
                "-look_ahead_depth",
                "100",
                "-low_delay_brc",
                "1",
                "-extbrc",
                "1",
                "-global_quality",
                "24",
                "-mbbrc",
                "1",
                "-adaptive_i",
                "1",
                "-adaptive_b",
                "1",
            ],
            GpuType::Cpu => vec![
                "-preset",
                "faster",
                "-profile:v",
                "high",
                "-level:v",
                "4.1",
                "-crf",
                "24",
            ],
        }
    }

    pub fn stage1_format_filter(self) -> &'static str {
        match self {
            GpuType::Nvidia => "scale_cuda=format=nv12",
            GpuType::Amd => "scale_vaapi=format=nv12",
            GpuType::Intel => "scale_qsv=format=nv12",
            GpuType::Cpu => "format=yuv420p",
        }
    }

    pub fn partial_upload_filters(self) -> Vec<&'static str> {
        match self {
            GpuType::Nvidia => vec!["format=nv12", "hwupload"],
            GpuType::Amd => vec!["format=nv12", "hwupload=derive_device=vaapi"],
            GpuType::Intel => vec!["format=nv12", "hwupload=extra_hw_frames=64,format=qsv"],
            GpuType::Cpu => vec![],
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            GpuType::Nvidia => "NVIDIA",
            GpuType::Amd => "AMD",
            GpuType::Intel => "Intel",
            GpuType::Cpu => "CPU",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GpuDevice, GpuType};

    #[test]
    fn friendly_model_bracketed() {
        let d = GpuDevice {
            gpu_type: GpuType::Nvidia,
            name: "NVIDIA Corporation TU116 [GeForce GTX 1660 Ti] (rev a1)".into(),
            device_path: String::new(),
        };
        assert_eq!(d.friendly_model(), "GeForce GTX 1660 Ti");
    }

    #[test]
    fn friendly_model_no_brackets() {
        let d = GpuDevice {
            gpu_type: GpuType::Intel,
            name: "Intel Corporation UHD Graphics 730 (rev 0c)".into(),
            device_path: String::new(),
        };
        assert_eq!(d.friendly_model(), "Intel UHD Graphics 730");
    }

    #[test]
    fn friendly_model_amd_cleanup() {
        let d = GpuDevice {
            gpu_type: GpuType::Amd,
            name: "Advanced Micro Devices, Inc. Navi 14 (rev c1)".into(),
            device_path: String::new(),
        };
        assert_eq!(d.friendly_model(), "AMD Navi 14");
    }

    #[test]
    fn vendor_name_from_lspci_string() {
        let d = GpuDevice {
            gpu_type: GpuType::Amd,
            name: "Intel Corporation UHD Graphics 730".into(),
            device_path: String::new(),
        };
        assert_eq!(d.vendor_name(), "Intel");
    }

    #[test]
    fn vendor_name_fallback_to_type() {
        let d = GpuDevice {
            gpu_type: GpuType::Nvidia,
            name: "Some Unknown Card".into(),
            device_path: String::new(),
        };
        assert_eq!(d.vendor_name(), "NVIDIA");
    }

    #[test]
    fn label_cpu() {
        let d = GpuDevice {
            gpu_type: GpuType::Cpu,
            name: "CPU (Software)".into(),
            device_path: String::new(),
        };
        assert_eq!(d.label(), "CPU (Software)");
    }

    #[test]
    fn label_nvidia_with_model() {
        let d = GpuDevice {
            gpu_type: GpuType::Nvidia,
            name: "NVIDIA Corporation TU116 [GeForce GTX 1660 Ti] (rev a1)".into(),
            device_path: "/dev/dri/renderD128".into(),
        };
        assert_eq!(d.label(), "NVIDIA (GeForce GTX 1660 Ti)");
    }

    #[test]
    fn h264_encoder_names() {
        assert_eq!(GpuType::Nvidia.h264_encoder(), "h264_nvenc");
        assert_eq!(GpuType::Amd.h264_encoder(), "h264_vaapi");
        assert_eq!(GpuType::Intel.h264_encoder(), "h264_qsv");
        assert_eq!(GpuType::Cpu.h264_encoder(), "libx264");
    }

    #[test]
    fn display_names() {
        assert_eq!(GpuType::Nvidia.display_name(), "NVIDIA");
        assert_eq!(GpuType::Amd.display_name(), "AMD");
        assert_eq!(GpuType::Intel.display_name(), "Intel");
        assert_eq!(GpuType::Cpu.display_name(), "CPU");
    }

    #[test]
    fn cpu_has_no_hwaccel_args() {
        assert!(GpuType::Cpu.hwaccel_decode_args().is_empty());
        assert!(GpuType::Cpu.hwaccel_args().is_empty());
        assert!(GpuType::Cpu.partial_gpu_args().is_empty());
        assert!(GpuType::Cpu.init_hw_args("").is_empty());
        assert!(GpuType::Cpu.partial_upload_filters().is_empty());
    }

    #[test]
    fn nvidia_init_hw_with_device() {
        let args = GpuType::Nvidia.init_hw_args("1");
        assert_eq!(args, vec!["-init_hw_device", "cuda=cu:1"]);
    }

    #[test]
    fn nvidia_init_hw_no_device() {
        let args = GpuType::Nvidia.init_hw_args("");
        assert_eq!(args, vec!["-init_hw_device", "cuda=cu:0"]);
    }

    #[test]
    fn intel_init_hw_needs_qsv_bridge() {
        let args = GpuType::Intel.init_hw_args("/dev/dri/renderD128");
        assert_eq!(args.len(), 4);
        assert!(args.contains(&"-init_hw_device".to_string()));
        assert!(args.iter().any(|a| a.contains("qsv")));
    }

    #[test]
    fn amd_vaapi_init_hw() {
        let args = GpuType::Amd.init_hw_args("/dev/dri/renderD129");
        assert_eq!(args.len(), 2);
        assert!(args[1].contains("vaapi"));
        assert!(args[1].contains("renderD129"));
    }

    #[test]
    fn scale_filters() {
        assert_eq!(GpuType::Nvidia.scale_filter(), "scale_cuda");
        assert_eq!(GpuType::Amd.scale_filter(), "scale_vaapi");
        assert_eq!(GpuType::Intel.scale_filter(), "scale_qsv");
        assert_eq!(GpuType::Cpu.scale_filter(), "scale");
    }

    #[test]
    fn quality_args_all_vendors_non_empty() {
        assert!(!GpuType::Nvidia.quality_args().is_empty());
        assert!(!GpuType::Amd.quality_args().is_empty());
        assert!(!GpuType::Intel.quality_args().is_empty());
        assert!(!GpuType::Cpu.quality_args().is_empty());
    }

    #[test]
    fn cpu_quality_uses_libx264_params() {
        let args = GpuType::Cpu.quality_args();
        assert!(args.contains(&"-crf"));
        assert!(args.contains(&"-preset"));
    }

    #[test]
    fn stage1_format_filter_all_nv12_or_yuv420p() {
        assert!(GpuType::Nvidia.stage1_format_filter().contains("nv12"));
        assert!(GpuType::Amd.stage1_format_filter().contains("nv12"));
        assert!(GpuType::Intel.stage1_format_filter().contains("nv12"));
        assert!(GpuType::Cpu.stage1_format_filter().contains("yuv420p"));
    }

    #[test]
    fn partial_upload_filters_hw_vendors_have_hwupload() {
        for gpu in [GpuType::Nvidia, GpuType::Amd, GpuType::Intel] {
            let filters = gpu.partial_upload_filters();
            assert!(
                filters.iter().any(|f| f.contains("hwupload")),
                "{gpu:?} should have hwupload filter"
            );
        }
    }
}
