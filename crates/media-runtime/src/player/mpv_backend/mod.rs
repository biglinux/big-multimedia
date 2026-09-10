//! mpv backend — wraps the system libmpv C API for audio/video playback with render API integration.
//!
//! Provides `MpvBackend` which manages an mpv instance behind a mutex,
//! supporting property observation, audio filter chains, and OpenGL rendering.

mod backend;
mod pactl_volume;
mod types;

pub use backend::{MpvBackend, MpvError};
pub use types::{
    AudioCacheProfile, AudioEffects, AudioEffectsSnapshot, AudioMetadata, PlayerEvent,
    TrackMetadata,
};

/// Diagnostic env-var toggle: set `SKIP_MPV_RENDER=1` to suppress all mpv
/// rendering calls. Used to isolate whether a VRAM leak originates in mpv
/// or GTK4/driver. Reads once and caches.
pub fn skip_mpv_render() -> bool {
    use std::sync::OnceLock;
    static SKIP: OnceLock<bool> = OnceLock::new();
    *SKIP.get_or_init(|| {
        let skip = std::env::var("SKIP_MPV_RENDER").is_ok();
        if skip {
            log::warn!("SKIP_MPV_RENDER is set — mpv rendering disabled (diagnostic mode)");
        }
        skip
    })
}

pub(crate) fn is_proprietary_nvidia_session() -> bool {
    use std::sync::OnceLock;
    static IS_NVIDIA: OnceLock<bool> = OnceLock::new();
    *IS_NVIDIA.get_or_init(|| {
        [
            "VK_DRIVER_FILES",
            "VK_ICD_FILENAMES",
            "__GLX_VENDOR_LIBRARY_NAME",
            "GBM_BACKEND",
            "LIBVA_DRIVER_NAME",
            "VDPAU_DRIVER",
        ]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .any(|value| env_value_names_nvidia_driver(&value))
    })
}

fn env_value_names_nvidia_driver(value: &str) -> bool {
    value.to_ascii_lowercase().contains("nvidia")
}

pub(crate) mod egl_dmabuf;
pub mod fast_render;
pub mod filters;
/// GPU-offloaded video renderer: DMA-BUF export (MESA) or GdkGLTexture
/// fallback → `GtkGraphicsOffload` → Wayland subsurface. Used by both the
/// video player and the video converter's edit page.
pub mod offload_renderer;
mod playback;
mod properties;
mod properties_core;
mod properties_rendering;
mod properties_subtitles;
mod properties_video;
pub mod shader_gen;
pub mod video_surface;

#[cfg(test)]
mod tests {
    use super::env_value_names_nvidia_driver;

    #[test]
    fn detects_nvidia_driver_env_values() {
        assert!(env_value_names_nvidia_driver(
            "/usr/share/vulkan/icd.d/nvidia_icd.json"
        ));
        assert!(env_value_names_nvidia_driver("nvidia"));
    }

    #[test]
    fn non_nvidia_driver_env_values_stay_eligible() {
        assert!(!env_value_names_nvidia_driver(
            "/usr/share/vulkan/icd.d/radeon_icd.x86_64.json"
        ));
        assert!(!env_value_names_nvidia_driver("iris"));
    }
}
