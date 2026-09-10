//! Zero-allocation render context wrapping the mpv render C API directly.
//! Uses stack-allocated render params for the hot frame path.

use std::ffi::c_void;

use crate::error::{Error, Result};

pub struct FastRenderContext {
    render_context: *mut mpv_sys::mpv_render_context,
    update_callback_cleanup: Option<Box<dyn FnOnce()>>,
}

// SAFETY: `mpv_render_context` is documented by libmpv as thread-compatible:
// it may be moved between threads as long as a single thread calls its
// non-`set_update_callback` API at a time. We never share a
// `FastRenderContext` across threads (no `Sync` impl), only transfer
// ownership. The optional `update_callback_cleanup` closure is itself
// `FnOnce + 'static` and `Send`-bound by GTK's main-context spawn API
// used at the call site, so moving the struct does not violate any
// internal pointer invariant.
unsafe impl Send for FastRenderContext {}

impl FastRenderContext {
    /// # Safety
    /// `mpv` must be a valid, non-null mpv handle that outlives the returned context.
    pub unsafe fn new(mpv: *mut mpv_sys::mpv_handle) -> Result<Self> {
        unsafe { Self::new_with_wayland_display(mpv, None, true) }
    }

    /// # Safety
    /// `mpv` must be a valid, non-null mpv handle that outlives the returned
    /// context. `wl_display`, when set, must stay valid for the render context.
    pub unsafe fn new_with_wayland_display(
        mpv: *mut mpv_sys::mpv_handle,
        wl_display: Option<*mut c_void>,
        advanced_control: bool,
    ) -> Result<Self> {
        let init_params = mpv_sys::mpv_opengl_init_params {
            get_proc_address: Some(gl_proc_address_raw),
            get_proc_address_ctx: std::ptr::null_mut(),
        };

        let api_type = b"opengl\0";
        // Advanced control enables mpv's threaded/offscreen render (needed by the
        // GraphicsOffload path). It DEADLOCKS on software GL (llvmpipe) in a
        // plain GtkGLArea, so the GLArea/no-GPU path uses simple mode — matching
        // Celluloid, which renders fine on llvmpipe.
        let mut advanced: i32 = i32::from(advanced_control);
        let mut params = [
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut c_void,
            },
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: std::ptr::addr_of!(init_params) as *mut c_void,
            },
            // Required for user shaders, FBO management, and full libplacebo pipeline.
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_ADVANCED_CONTROL,
                data: std::ptr::addr_of_mut!(advanced).cast(),
            },
            mpv_sys::mpv_render_param {
                type_: wl_display.map_or(0, |_| {
                    mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_WL_DISPLAY
                }),
                data: wl_display.unwrap_or(std::ptr::null_mut()),
            },
            mpv_sys::mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            },
        ];

        let mut render_context: *mut mpv_sys::mpv_render_context = std::ptr::null_mut();
        // SAFETY: handle is valid; init_params and api_type live for this call.
        let err = unsafe {
            mpv_sys::mpv_render_context_create(&raw mut render_context, mpv, params.as_mut_ptr())
        };

        if err < 0 {
            return Err(Error::mpv(format!(
                "mpv_render_context_create failed: {err}"
            )));
        }

        Ok(Self {
            render_context,
            update_callback_cleanup: None,
        })
    }

    /// Returns 0 on success, negative mpv error code on failure.
    #[inline]
    pub fn render(&self, fbo: i32, width: i32, height: i32, flip_y: bool) -> i32 {
        let mut fbo_struct = mpv_sys::mpv_opengl_fbo {
            fbo,
            w: width,
            h: height,
            internal_format: 0, // 0 = unknown — mpv uses the fbo-format option.
        };
        let mut flip_val: i32 = i32::from(flip_y);

        let mut params = [
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO,
                data: std::ptr::addr_of_mut!(fbo_struct).cast(),
            },
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y,
                data: std::ptr::addr_of_mut!(flip_val).cast(),
            },
            mpv_sys::mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            },
        ];

        // SAFETY: render_context valid; params valid for the synchronous call duration.
        unsafe { mpv_sys::mpv_render_context_render(self.render_context, params.as_mut_ptr()) }
    }

    /// Improves mpv A/V timing.
    #[inline]
    pub fn report_swap(&self) {
        // SAFETY: render_context valid; report_swap has no preconditions.
        unsafe { mpv_sys::mpv_render_context_report_swap(self.render_context) }
    }

    /// `true` when `render()` would produce a different image.
    #[inline]
    pub fn has_new_frame(&self) -> bool {
        // SAFETY: render_context valid; mpv_render_context_update is thread-safe.
        let flags = unsafe { mpv_sys::mpv_render_context_update(self.render_context) };
        (flags & u64::from(mpv_sys::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME)) != 0
    }

    /// Callback fires from an mpv internal thread — do NOT call mpv API from it.
    /// Schedule work on the main thread instead (e.g. `glib::idle_add_once`).
    pub fn set_update_callback<F: Fn() + Send + 'static>(&mut self, callback: F) {
        if let Some(cleanup) = self.update_callback_cleanup.take() {
            cleanup();
        }

        let raw_callback = Box::into_raw(Box::new(callback));
        self.update_callback_cleanup = Some(Box::new(move || {
            // SAFETY: raw_callback came from Box::into_raw above.
            unsafe { drop(Box::from_raw(raw_callback)) };
        }));

        // SAFETY: raw_callback points to a valid F that lives until cleanup.
        unsafe {
            mpv_sys::mpv_render_context_set_update_callback(
                self.render_context,
                Some(update_wrapper::<F>),
                raw_callback.cast(),
            );
        }
    }
}

impl Drop for FastRenderContext {
    fn drop(&mut self) {
        // Unregister before dropping the callback box so mpv never observes a
        // stale callback pointer during render-context teardown.
        unsafe {
            mpv_sys::mpv_render_context_set_update_callback(
                self.render_context,
                None,
                std::ptr::null_mut(),
            );
        }
        if let Some(cleanup) = self.update_callback_cleanup.take() {
            cleanup();
        }
        // SAFETY: render_context came from new() and is freed exactly once here.
        unsafe {
            mpv_sys::mpv_render_context_free(self.render_context);
        }
    }
}

/// Software (CPU) render context — the no-GL fallback. mpv renders each frame
/// into a caller-supplied `bgr0` (B,G,R,unused) byte buffer instead of an FBO,
/// so the player can show video on hosts/compositors without an OpenGL context
/// (e.g. headless software compositors, GL-less VMs). Pairs with
/// `gdk::MemoryFormat::B8g8r8x8`.
pub struct SwRenderContext {
    render_context: *mut mpv_sys::mpv_render_context,
    update_callback_cleanup: Option<Box<dyn FnOnce()>>,
}

// SAFETY: same thread-compatibility contract as `FastRenderContext` — ownership
// is transferred between threads, never shared (no `Sync`).
unsafe impl Send for SwRenderContext {}

impl SwRenderContext {
    /// # Safety
    /// `mpv` must be a valid, non-null mpv handle that outlives the context.
    pub unsafe fn new(mpv: *mut mpv_sys::mpv_handle) -> Result<Self> {
        let api_type = b"sw\0";
        let mut params = [
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut c_void,
            },
            mpv_sys::mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            },
        ];

        let mut render_context: *mut mpv_sys::mpv_render_context = std::ptr::null_mut();
        // SAFETY: handle valid; api_type lives for this call.
        let err = unsafe {
            mpv_sys::mpv_render_context_create(&raw mut render_context, mpv, params.as_mut_ptr())
        };
        if err < 0 {
            return Err(Error::mpv(format!(
                "mpv_render_context_create (sw) failed: {err}"
            )));
        }
        Ok(Self {
            render_context,
            update_callback_cleanup: None,
        })
    }

    /// Render the current frame into `buffer` as `bgr0` pixels. `buffer` must be
    /// at least `stride * height` bytes. Returns 0 on success, negative on error.
    #[inline]
    pub fn render_into(&self, buffer: &mut [u8], width: i32, height: i32, stride: usize) -> i32 {
        if width <= 0 || height <= 0 || buffer.len() < stride.saturating_mul(height as usize) {
            return -1;
        }
        let mut size: [i32; 2] = [width, height];
        let format = b"bgr0\0";
        let mut stride_val: usize = stride;
        let mut params = [
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_SIZE,
                data: size.as_mut_ptr().cast(),
            },
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_FORMAT,
                data: format.as_ptr() as *mut c_void,
            },
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_STRIDE,
                data: std::ptr::addr_of_mut!(stride_val).cast(),
            },
            mpv_sys::mpv_render_param {
                type_: mpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_POINTER,
                data: buffer.as_mut_ptr().cast(),
            },
            mpv_sys::mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            },
        ];
        // SAFETY: render_context valid; all param data outlive the synchronous call.
        unsafe { mpv_sys::mpv_render_context_render(self.render_context, params.as_mut_ptr()) }
    }

    /// `true` when a new frame is available to render.
    #[inline]
    pub fn has_new_frame(&self) -> bool {
        // SAFETY: render_context valid; update query is thread-safe.
        let flags = unsafe { mpv_sys::mpv_render_context_update(self.render_context) };
        (flags & u64::from(mpv_sys::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME)) != 0
    }

    /// Callback fires from an mpv internal thread — schedule work on the main
    /// thread (e.g. `glib::idle_add_once`), never call mpv API from it.
    pub fn set_update_callback<F: Fn() + Send + 'static>(&mut self, callback: F) {
        if let Some(cleanup) = self.update_callback_cleanup.take() {
            cleanup();
        }
        let raw_callback = Box::into_raw(Box::new(callback));
        self.update_callback_cleanup = Some(Box::new(move || {
            // SAFETY: raw_callback came from Box::into_raw above.
            unsafe { drop(Box::from_raw(raw_callback)) };
        }));
        // SAFETY: raw_callback points to a valid F that lives until cleanup.
        unsafe {
            mpv_sys::mpv_render_context_set_update_callback(
                self.render_context,
                Some(update_wrapper::<F>),
                raw_callback.cast(),
            );
        }
    }
}

impl Drop for SwRenderContext {
    fn drop(&mut self) {
        unsafe {
            mpv_sys::mpv_render_context_set_update_callback(
                self.render_context,
                None,
                std::ptr::null_mut(),
            );
        }
        if let Some(cleanup) = self.update_callback_cleanup.take() {
            cleanup();
        }
        // SAFETY: render_context came from new() and is freed exactly once here.
        unsafe {
            mpv_sys::mpv_render_context_free(self.render_context);
        }
    }
}

/// SAFETY: `render_context` must point to a valid `F` created by `Box::into_raw`. mpv
/// guarantees the callback is not called after the context is freed or a
/// different callback is set.
unsafe extern "C" fn update_wrapper<F: Fn() + Send + 'static>(render_context: *mut c_void) {
    unsafe { (*(render_context as *const F))() }
}

/// SAFETY: `name` is a valid null-terminated string from mpv. `dlsym` with
/// `RTLD_DEFAULT` is safe for resolving GL function pointers.
unsafe extern "C" fn gl_proc_address_raw(
    _ctx: *mut c_void,
    name: *const std::ffi::c_char,
) -> *mut c_void {
    unsafe { libc::dlsym(libc::RTLD_DEFAULT, name) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_param_array_is_stack_allocated() {
        assert_eq!(
            std::mem::size_of::<mpv_sys::mpv_render_param>(),
            16,
            "mpv_render_param should be 16 bytes (type u32 + padding + pointer)"
        );
        assert_eq!(
            std::mem::size_of::<mpv_sys::mpv_opengl_fbo>(),
            16,
            "mpv_opengl_fbo should be 16 bytes (4 x i32)"
        );
    }

    #[test]
    fn fast_render_context_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<FastRenderContext>();
    }
}
