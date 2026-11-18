//! Public `VideoBackend` enum and detection logic.

use gtk::gdk;
use log::info;

use super::OffloadRenderer;

pub enum VideoBackend {
    /// GdkDmabufTexture/GdkGLTexture + GraphicsOffload (Wayland subsurface when available).
    Offload(Box<OffloadRenderer>),
    /// GtkGLArea fallback.
    GlArea,
}

impl VideoBackend {
    /// Tries offload first (best on Wayland, harmless on X11), falls back to GLArea.
    pub fn detect(display: &gdk::Display) -> Self {
        if !Self::graphics_offload_requested() {
            // allow-noisy-log: one line per video surface, naming its backend.
            info!("GraphicsOffload video backend not requested");
            return Self::GlArea;
        }

        if let Some(renderer) = OffloadRenderer::try_new(display) {
            let mode = if renderer.has_egl_dmabuf() {
                "DMA-BUF + GraphicsOffload"
            } else {
                "GdkGLTexture + GraphicsOffload"
            };
            info!("Using offload video backend ({mode})");
            return Self::Offload(Box::new(renderer));
        }

        info!("Using GLArea video backend (fallback)");
        Self::GlArea
    }

    /// Offload only when `BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD` is set and not
    /// truthy: it costs the GL area's CPU plus an idle GL context and texture pool.
    fn graphics_offload_requested() -> bool {
        std::env::var("BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD").is_ok_and(|v| {
            !matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
    }

    pub fn is_offload(&self) -> bool {
        matches!(self, Self::Offload(_))
    }
}
