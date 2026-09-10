//! Public `VideoBackend` enum and detection logic. Split out of
//! `offload_renderer.rs`; behavior is identical.

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
        if Self::graphics_offload_disabled() {
            info!("GraphicsOffload video backend disabled by environment");
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

    /// `BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD` as a boolean: truthy (`1`/`true`/
    /// `yes`/`on`) disables the offload backend; falsy (`0`/`false`/`no`/`off`/
    /// empty) or unset leaves it enabled. A real toggle so an app can default it
    /// off yet let the user re-enable (offload zero-copy is great where DMA-BUF
    /// import works, but on GPUs that export non-linear modifiers it falls back
    /// to a cross-context GL-texture path that is slower than the GLArea path).
    fn graphics_offload_disabled() -> bool {
        std::env::var("BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD").is_ok_and(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
    }

    pub fn is_offload(&self) -> bool {
        matches!(self, Self::Offload(_))
    }
}

#[cfg(test)]
mod tests {
    use super::super::gl_fns::{GL_FRAMEBUFFER_COMPLETE, GL_TEXTURE_2D};

    #[test]
    fn offload_renderer_consts_valid() {
        assert_eq!(GL_TEXTURE_2D, 0x0DE1);
        assert_eq!(GL_FRAMEBUFFER_COMPLETE, 0x8CD5);
    }
}
