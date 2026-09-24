#[cfg(not(miri))]
use std::time::Duration;

use super::types::{GpuDevice, GpuType};
#[cfg(not(miri))]
use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
#[cfg(not(miri))]
use std::path::Path;

/// Detect working H264 encoders. Call from a worker: each driver probe has a
/// five-second deadline. Re-enumerating lets a new job see device/driver changes.
pub fn detect_all_gpus() -> Vec<GpuDevice> {
    detect_all_gpus_inner()
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

#[cfg(any(test, not(miri)))]
fn parse_cuda_devices(output: &str) -> Vec<GpuDevice> {
    let mut devices: Vec<GpuDevice> = Vec::new();
    for line in output.lines() {
        let Some((_, record)) = line.split_once("[ GPU #") else {
            continue;
        };
        let Some((ordinal, model)) = record.split_once(" - < ") else {
            continue;
        };
        let Some((name, _)) = model.split_once(" >") else {
            continue;
        };
        let Ok(ordinal) = ordinal.parse::<u32>() else {
            continue;
        };
        let ordinal = ordinal.to_string();
        if !name.trim().is_empty() && !devices.iter().any(|d| d.device_path == ordinal) {
            devices.push(GpuDevice {
                gpu_type: GpuType::Nvidia,
                name: name.trim().to_string(),
                device_path: ordinal,
            });
        }
    }
    devices
}

#[cfg(not(miri))]
fn drm_devices(sysfs: &Path, pci_listing: &str) -> Vec<GpuDevice> {
    let Ok(entries) = std::fs::read_dir(sysfs) else {
        return Vec::new();
    };
    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name
            .strip_prefix("renderD")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        let device = entry.path().join("device");
        let vendor = std::fs::read_to_string(device.join("vendor")).unwrap_or_default();
        let gpu_type = match vendor.trim() {
            "0x8086" => GpuType::Intel,
            "0x1002" => GpuType::Amd,
            _ => continue,
        };
        let pci_address = device
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let model = pci_address
            .as_deref()
            .and_then(|address| {
                pci_listing.lines().find_map(|line| {
                    let (id, description) = line.split_once(' ')?;
                    (id == address).then(|| {
                        description
                            .split_once(": ")
                            .map_or(description, |(_, name)| name)
                            .trim()
                            .to_string()
                    })
                })
            })
            .unwrap_or_else(|| gpu_type.display_name().into());
        devices.push(GpuDevice {
            gpu_type,
            name: model,
            device_path: format!("/dev/dri/{name}"),
        });
    }
    devices.sort_by(|a, b| a.device_path.cmp(&b.device_path));
    devices
}

#[cfg(not(miri))]
fn encoder_works(device: &GpuDevice) -> bool {
    let output = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-filter_threads",
            "1",
        ])
        .args(device.gpu_type.init_hw_args(&device.device_path))
        .args(device.gpu_type.partial_gpu_args())
        .args(["-f", "lavfi", "-i", "color=size=640x360:rate=30", "-vf"])
        .arg(device.gpu_type.partial_upload_filters().join(","))
        .args([
            "-c:v",
            device.gpu_type.h264_encoder(),
            "-frames:v",
            "2",
            "-f",
            "null",
            "-",
        ])
        .stdout(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(5))
        .build()
        .run();
    let working = output.is_ok_and(|o| o.status.success());
    if !working {
        log::info!(
            "H264 encoder unavailable on {} ({:?})",
            device.device_path,
            device.gpu_type
        );
    }
    working
}

#[cfg(not(miri))]
fn detect_all_gpus_inner() -> Vec<GpuDevice> {
    let pci_listing = BigSubprocessSpec::builder()
        .program("lspci")
        .arg("-D")
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut candidates = drm_devices(Path::new("/sys/class/drm"), &pci_listing);
    // FFmpeg lists CUDA ordinals, which can differ from PCI, DRM and
    // nvidia-smi ordering (including CUDA_VISIBLE_DEVICES remapping).
    // The listing intentionally exits nonzero after printing the devices.
    if let Ok(output) = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "info",
            "-f",
            "lavfi",
            "-i",
            "color=size=640x360:rate=30",
            "-c:v",
            "h264_nvenc",
            "-gpu",
            "list",
            "-frames:v",
            "1",
            "-f",
            "null",
            "-",
        ])
        .stdout(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(5))
        .build()
        .run()
    {
        let mut cuda = parse_cuda_devices(&String::from_utf8_lossy(&output.stderr));
        cuda.append(&mut candidates);
        candidates = cuda;
    }
    let mut devices = Vec::new();
    for mut device in candidates {
        if encoder_works(&device) {
            devices.push(device);
        } else if device.gpu_type == GpuType::Intel {
            // Intel hardware can support VAAPI even when the QSV driver fails.
            device.gpu_type = GpuType::Amd;
            if encoder_works(&device) {
                devices.push(device);
            }
        }
    }
    devices.push(software_gpu_device());
    devices
}

/// Identifier of the first working hardware encoder, or software fallback.
pub fn detect_gpu_device() -> (GpuType, String) {
    let device = detect_all_gpus()
        .into_iter()
        .next()
        .unwrap_or_else(software_gpu_device);
    (device.gpu_type, device.device_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUDA_CORPUS: &str = include_str!("cuda_devices.corpus");

    #[test]
    fn cuda_ordinals_are_not_drm_or_pci_indices() {
        let devices = parse_cuda_devices(CUDA_CORPUS);
        assert_eq!(
            devices
                .iter()
                .map(|d| d.device_path.as_str())
                .collect::<Vec<_>>(),
            ["0", "2"]
        );
        assert_eq!(devices[0].name, "NVIDIA GeForce GTX 1660 Ti");
        assert_eq!(devices[1].name, "NVIDIA GPU: secondary");
    }

    #[test]
    #[cfg(not(miri))]
    fn drm_identity_uses_pci_address_and_all_render_nodes() {
        use std::{fs, os::unix::fs::symlink};
        let tmp = tempfile::tempdir().unwrap();
        let drm = tmp.path().join("drm");
        fs::create_dir(&drm).unwrap();
        for (node, pci, vendor) in [
            ("renderD150", "0000:01:00.0", "0x8086"),
            ("renderD128", "0000:05:00.0", "0x1002"),
        ] {
            let device = tmp.path().join(pci);
            fs::create_dir(&device).unwrap();
            fs::write(device.join("vendor"), vendor).unwrap();
            fs::create_dir(drm.join(node)).unwrap();
            symlink(device, drm.join(node).join("device")).unwrap();
        }
        let devices = drm_devices(
            &drm,
            "0000:01:00.0 VGA compatible controller: Intel Arc\n0000:05:00.0 Display controller: AMD Radeon\n",
        );
        assert_eq!(devices[0].name, "AMD Radeon");
        assert_eq!(devices[0].device_path, "/dev/dri/renderD128");
        assert_eq!(devices[1].name, "Intel Arc");
        assert_eq!(devices[1].device_path, "/dev/dri/renderD150");
    }

    #[cfg(feature = "fuzz")]
    mod fuzz {
        use super::*;
        use proptest::prelude::*;
        proptest! {
            #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
            #[test]
            fn cuda_output_fuzz(bytes in proptest::collection::vec(any::<u8>(), 0..16_384)) {
                let output = format!("{CUDA_CORPUS}{}", String::from_utf8_lossy(&bytes));
                let devices = parse_cuda_devices(&output);
                let mut ids = std::collections::HashSet::new();
                for device in devices {
                    prop_assert!(device.device_path.parse::<u32>().is_ok());
                    prop_assert!(ids.insert(device.device_path));
                    prop_assert!(!device.name.is_empty());
                }
            }
        }
    }
}
