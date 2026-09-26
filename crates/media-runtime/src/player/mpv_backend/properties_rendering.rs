//! Rendering & GPU: hardware decoding, GPU API, ICC color management,
//! render context, sharpen, deband, auto-window-resize, screensaver, render profiles.

use log::info;

use super::MpvBackend;
use big_mpv_render::{MpvPropertyValue, render_profile_properties};

impl MpvBackend {
    /// Profiles: `"default"` (lean/fast — bilinear, audio sync),
    /// `"balanced"` (lanczos, correct-downscaling),
    /// `"high-quality"` (ewa_lanczossharp, display-resample, interpolation).
    pub fn apply_render_profile(&self, profile: &str) {
        info!("Applying render profile: {profile}");
        self.with_mpv(|m| {
            for property in render_profile_properties(profile) {
                match property.value {
                    MpvPropertyValue::Text(value) => {
                        let _ = m.set_property(property.name, value.to_string());
                    }
                    MpvPropertyValue::Bool(value) => {
                        let _ = m.set_property(property.name, value);
                    }
                }
            }
        });
    }

    pub fn set_hwdec(&self, mode: &str) {
        self.with_mpv(|m| {
            let mut filters = self
                .hwdec_before_filters
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let effective = if let Some(filters) = filters.as_mut() {
                *filters = mode.to_string();
                super::filters::software_hwdec(mode)
            } else {
                mode.to_string()
            };
            let _ = m.set_property("hwdec", effective);
        });
    }

    pub fn get_hwdec(&self) -> String {
        self.with_mpv(|m| {
            self.hwdec_before_filters
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .map_or_else(
                    || m.get_property::<String>("hwdec").unwrap_or_default(),
                    Clone::clone,
                )
        })
    }

    pub fn set_ytdl_format(&self, format: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("ytdl-format", format.to_string());
        });
    }

    pub fn set_sharpen(&self, val: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sharpen", val);
        });
    }

    pub fn set_deband(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "deband",
                if enable {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    pub fn set_sigmoid_upscaling(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "sigmoid-upscaling",
                if enable {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    pub fn set_correct_downscaling(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "correct-downscaling",
                if enable {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    pub fn set_icc_profile(&self, path: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("icc-profile", path.to_string());
        });
    }

    pub fn set_icc_intent(&self, intent: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("icc-3dlut-size", "64x64x64".to_string());
            let _ = m.set_property("icc-intent", intent);
        });
    }

    /// Advanced control requires servicing every update callback on the render
    /// thread, with its GL context current. It does not enable shaders.
    ///
    /// # Safety
    /// `wl_display`, when present, must be a live Wayland display for the entire
    /// render-context lifetime. The creation GL context must be current for
    /// render API calls and teardown. A raw native pointer cannot be accepted
    /// through a safe API merely because the mpv handle itself is retained.
    pub unsafe fn create_fast_render_context_with_wayland_display(
        self: &std::sync::Arc<Self>,
        wl_display: Option<*mut std::ffi::c_void>,
        advanced_control: bool,
    ) -> crate::error::Result<super::fast_render::FastRenderContext> {
        // SAFETY: with_owner retains this backend until AFTER the C render
        // context is freed. The caller provides the current GL/display context.
        unsafe {
            super::fast_render::FastRenderContext::new_with_wayland_display(
                self.raw_mpv_handle,
                wl_display,
                advanced_control,
            )
        }
        .map(|context| context.with_owner(std::sync::Arc::clone(self)))
    }

    /// Software fallback, retaining the C mpv handle for the context lifetime.
    pub fn create_sw_render_context(
        self: &std::sync::Arc<Self>,
    ) -> crate::error::Result<super::fast_render::SwRenderContext> {
        // SAFETY: the returned context owns a strong backend reference.
        unsafe { super::fast_render::SwRenderContext::new(self.raw_mpv_handle) }
            .map(|context| context.with_owner(std::sync::Arc::clone(self)))
    }
}
