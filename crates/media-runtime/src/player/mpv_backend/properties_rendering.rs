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
            let _ = m.set_property("hwdec", mode.to_string());
        });
    }

    pub fn get_hwdec(&self) -> String {
        self.with_mpv(|m| m.get_property::<String>("hwdec").unwrap_or_default())
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

    pub fn set_gpu_api(&self, api: &str) {
        self.with_mpv(|m| {
            if !api.is_empty() {
                let _ = m.set_property("gpu-api", api.to_string());
            }
        });
    }

    pub fn set_gpu_context(&self, gpu_context: &str) {
        self.with_mpv(|m| {
            if !gpu_context.is_empty() {
                let _ = m.set_property("gpu-context", gpu_context.to_string());
            }
        });
    }

    pub fn set_icc_profile(&self, path: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("icc-profile", path.to_string());
        });
    }

    pub fn set_icc_profile_auto(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("icc-profile-auto", enable);
        });
    }

    pub fn set_icc_intent(&self, intent: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("icc-3dlut-size", "64x64x64".to_string());
            let _ = m.set_property("icc-intent", intent);
        });
    }

    pub fn set_stop_screensaver(&self, mode: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("stop-screensaver", mode.to_string());
        });
    }

    pub fn set_auto_window_resize(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "auto-window-resize",
                if enable {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    /// Stack-allocated params for the mpv render API hot path.
    pub fn create_fast_render_context(
        &self,
    ) -> crate::error::Result<super::fast_render::FastRenderContext> {
        self.create_fast_render_context_with_wayland_display(None, true)
    }

    /// `advanced_control` enables mpv's threaded render (for GraphicsOffload);
    /// pass `false` for the plain GLArea path (it deadlocks on software GL).
    pub fn create_fast_render_context_with_wayland_display(
        &self,
        wl_display: Option<*mut std::ffi::c_void>,
        advanced_control: bool,
    ) -> crate::error::Result<super::fast_render::FastRenderContext> {
        // SAFETY: raw_mpv_handle outlives the render context — MpvBackend owns both.
        unsafe {
            super::fast_render::FastRenderContext::new_with_wayland_display(
                self.raw_mpv_handle,
                wl_display,
                advanced_control,
            )
        }
    }

    /// Software (CPU) render context — the no-GL fallback for hosts without an
    /// OpenGL context. Renders frames into a byte buffer instead of an FBO.
    pub fn create_sw_render_context(
        &self,
    ) -> crate::error::Result<super::fast_render::SwRenderContext> {
        // SAFETY: raw_mpv_handle outlives the render context — MpvBackend owns both.
        unsafe { super::fast_render::SwRenderContext::new(self.raw_mpv_handle) }
    }
}
