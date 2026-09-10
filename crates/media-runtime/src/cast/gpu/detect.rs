use std::sync::OnceLock;
use std::time::Duration;

#[cfg(not(miri))]
use std::path::Path;

#[cfg(not(miri))]
use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
#[cfg(not(miri))]
use log::info;

#[cfg(not(miri))]
use super::ffmpeg::ffmpeg_has_encoder;
use super::types::{GpuDevice, GpuType};

#[cfg(not(miri))]
fn vainfo_supports_h264_encode(device_path: &str) -> bool {
    let output = BigSubprocessSpec::builder()
        .program("vainfo")
        .args(["--display", "drm", "--device", device_path])
        .timeout(Duration::from_secs(5))
        .build()
        .run();
    match output {
        Ok(o) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            text.contains("VAProfileH264") && text.contains("VAEntrypointEncSlice")
        }
        Err(_) => false,
    }
}

#[cfg(any(test, not(miri)))]
fn classify_gpu(name: &str) -> GpuType {
    let low = name.to_lowercase();
    if low.contains("nvidia") {
        GpuType::Nvidia
    } else if low.contains("intel") {
        GpuType::Intel
    } else if low.contains("amd") || low.contains("ati") || low.contains("advanced micro") {
        GpuType::Amd
    } else {
        GpuType::Cpu
    }
}

static ALL_GPUS: OnceLock<Vec<GpuDevice>> = OnceLock::new();

/// Detect all available GPU devices for FFmpeg transcoding (cached).
pub fn detect_all_gpus() -> Vec<GpuDevice> {
    ALL_GPUS.get_or_init(detect_all_gpus_inner).clone()
}

fn software_gpu_device() -> GpuDevice {
    GpuDevice {
        gpu_type: GpuType::Cpu,
        name: "CPU (Software)".into(),
        device_path: String::new(),
    }
}

#[cfg(miri)]
fn detect_all_gpus_inner() -> Vec<GpuDevice> {
    vec![software_gpu_device()]
}

#[cfg(not(miri))]
fn detect_all_gpus_inner() -> Vec<GpuDevice> {
    let mut devices = Vec::new();

    let lspci_output = BigSubprocessSpec::builder()
        .program("lspci")
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let mut gpu_entries: Vec<(GpuType, String)> = Vec::new();
    for line in lspci_output.lines() {
        let low = line.to_lowercase();
        if (low.contains("vga") || low.contains("display") || low.contains("3d"))
            && let Some(name_part) = line.split(':').nth(2)
        {
            let name = name_part.trim().to_string();
            let gtype = classify_gpu(&name);
            if gtype != GpuType::Cpu {
                gpu_entries.push((gtype, name));
            }
        }
    }

    let render_nodes: Vec<String> = (128..136)
        .map(|n| format!("/dev/dri/renderD{n}"))
        .filter(|p| Path::new(p).exists())
        .collect();

    let has_nvidia = BigSubprocessSpec::builder()
        .program("nvidia-smi")
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .is_ok_and(|o| o.status.success());

    for (i, entry) in gpu_entries.iter().enumerate() {
        let (gtype, name) = entry;
        let device_path = render_nodes.get(i).cloned().unwrap_or_default();

        match gtype {
            GpuType::Nvidia => {
                if has_nvidia && ffmpeg_has_encoder("h264_nvenc") {
                    info!("Cast GPU: NVIDIA detected (h264_nvenc) at {device_path}");
                    devices.push(GpuDevice {
                        gpu_type: GpuType::Nvidia,
                        name: name.clone(),
                        device_path,
                    });
                }
            }
            GpuType::Intel => {
                let has_qsv = ffmpeg_has_encoder("h264_qsv");
                info!("Cast GPU: Intel check — h264_qsv={has_qsv} at {device_path}");
                if has_qsv {
                    devices.push(GpuDevice {
                        gpu_type: GpuType::Intel,
                        name: name.clone(),
                        device_path,
                    });
                } else if !device_path.is_empty()
                    && vainfo_supports_h264_encode(&device_path)
                    && ffmpeg_has_encoder("h264_vaapi")
                {
                    info!("Cast GPU: Intel using VAAPI fallback at {device_path}");
                    devices.push(GpuDevice {
                        gpu_type: GpuType::Amd,
                        name: name.clone(),
                        device_path,
                    });
                }
            }
            GpuType::Amd => {
                if !device_path.is_empty()
                    && vainfo_supports_h264_encode(&device_path)
                    && ffmpeg_has_encoder("h264_vaapi")
                {
                    devices.push(GpuDevice {
                        gpu_type: GpuType::Amd,
                        name: name.clone(),
                        device_path,
                    });
                }
            }
            GpuType::Cpu => {}
        }
    }

    devices.push(software_gpu_device());

    for dev in &devices {
        info!(
            "Cast GPU detected: {} ({:?}) at {}",
            dev.name, dev.gpu_type, dev.device_path
        );
    }

    devices
}

/// Detect the best available GPU for FFmpeg transcoding (auto-select).
pub fn detect_gpu() -> GpuType {
    let gpus = detect_all_gpus();
    gpus.iter()
        .find(|g| g.gpu_type != GpuType::Cpu)
        .map_or(GpuType::Cpu, |g| g.gpu_type)
}

/// Get the device path of the best auto-detected GPU.
pub fn detect_gpu_device() -> (GpuType, String) {
    let gpus = detect_all_gpus();
    gpus.iter()
        .find(|g| g.gpu_type != GpuType::Cpu)
        .map_or((GpuType::Cpu, String::new()), |g| {
            (g.gpu_type, g.device_path.clone())
        })
}

#[cfg(test)]
mod tests {
    use super::classify_gpu;
    use crate::cast::gpu::GpuType;

    #[test]
    fn classify_nvidia() {
        assert_eq!(classify_gpu("NVIDIA Corporation TU116"), GpuType::Nvidia);
    }

    #[test]
    fn classify_amd() {
        assert_eq!(
            classify_gpu("Advanced Micro Devices [Navi 14]"),
            GpuType::Amd
        );
    }

    #[test]
    fn classify_ati() {
        assert_eq!(classify_gpu("ATI Radeon HD 7900"), GpuType::Amd);
    }

    #[test]
    fn classify_intel() {
        assert_eq!(
            classify_gpu("Intel Corporation UHD Graphics 730"),
            GpuType::Intel
        );
    }

    #[test]
    fn classify_unknown_falls_back_to_cpu() {
        assert_eq!(classify_gpu("Matrox Electronics G200eR2"), GpuType::Cpu);
    }
}
