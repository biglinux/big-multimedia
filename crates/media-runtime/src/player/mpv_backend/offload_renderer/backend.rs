//! Public `VideoBackend` enum and detection logic.

use gtk::gdk;
use log::info;

use super::OffloadRenderer;

/// Truthy draws video through a GL area instead of graphics offload.
const DISABLE_GRAPHICS_OFFLOAD: &str = "BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD";

/// Test switch: truthy asks for graphics offload even where it is refused,
/// to measure the refusal.
pub(super) const FORCE_GRAPHICS_OFFLOAD: &str = "BIG_MEDIA_FORCE_GRAPHICS_OFFLOAD";

/// The renderer big-compositor's session was started with.
const COMPOSITOR_RENDERER: &str = "BIG_COMPOSITOR_RENDERER";

/// big-compositor composing with Pixman, its session without graphics
/// acceleration: an offloaded frame gets no plane there and costs the
/// compositor more than the GL area costs the player.
const SOFTWARE_COMPOSITOR: &str = "2d";

pub enum VideoBackend {
    /// GdkDmabufTexture/GdkGLTexture + GraphicsOffload (Wayland subsurface when available).
    Offload(Box<OffloadRenderer>),
    /// GtkGLArea fallback.
    GlArea,
}

impl VideoBackend {
    /// Tries offload first (best on Wayland, harmless on X11), falls back to GLArea.
    pub fn detect(display: &gdk::Display) -> Self {
        let forced = std::env::var(FORCE_GRAPHICS_OFFLOAD).is_ok_and(|v| truthy(&v));
        let disabled = std::env::var(DISABLE_GRAPHICS_OFFLOAD).is_ok_and(|v| truthy(&v));
        let software_compositor = std::env::var(COMPOSITOR_RENDERER)
            .is_ok_and(|renderer| renderer == SOFTWARE_COMPOSITOR);
        if !offload_requested(forced, disabled, software_compositor) {
            let reason = if disabled {
                format!("{DISABLE_GRAPHICS_OFFLOAD} is set")
            } else {
                format!(
                    "big-compositor composes in software \
                     ({COMPOSITOR_RENDERER}={SOFTWARE_COMPOSITOR})"
                )
            };
            // allow-noisy-log: the chosen video path is said once per player.
            info!("Using GLArea video backend: {reason}");
            return Self::GlArea;
        }

        if let Some(renderer) = OffloadRenderer::try_new(display, forced) {
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

    pub fn is_offload(&self) -> bool {
        matches!(self, Self::Offload(_))
    }
}

/// Whether the environment lets the player try offload before it knows the GL
/// driver: unless opted out or in big-compositor's software session, both of
/// which `forced` overrides.
fn offload_requested(forced: bool, disabled: bool, software_compositor: bool) -> bool {
    forced || !(disabled || software_compositor)
}

/// `1`, `true`, `yes` or `on`, in any case and surrounded by spaces.
fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    /// The environment check, then the driver check `try_new` makes, as the
    /// player runs them.
    #[test]
    fn offload_default_honors_refusals() {
        for (proprietary_nvidia, software_compositor, disabled, forced, offload) in [
            (false, false, false, false, true),
            (false, false, true, false, false),
            (false, true, false, false, false),
            (false, true, true, false, false),
            (true, false, false, false, false),
            (true, false, true, false, false),
            (true, true, false, false, false),
            (true, true, true, false, false),
            (false, false, false, true, true),
            (false, false, true, true, true),
            (false, true, false, true, true),
            (false, true, true, true, true),
            (true, false, false, true, true),
            (true, false, true, true, true),
            (true, true, false, true, true),
            (true, true, true, true, true),
        ] {
            assert_eq!(
                super::offload_requested(forced, disabled, software_compositor)
                    && super::super::offload_permitted(proprietary_nvidia, forced),
                offload,
                "proprietary NVIDIA {proprietary_nvidia}, software compositor \
                 {software_compositor}, opted out {disabled}, forced {forced}"
            );
        }
    }
}
