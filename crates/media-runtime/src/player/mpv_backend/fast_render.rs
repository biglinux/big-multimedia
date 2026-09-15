//! Zero-allocation render context wrapping the mpv render C API directly.
//! Uses stack-allocated render params for the hot frame path.

use std::ffi::c_void;
use std::sync::Arc;

use super::MpvBackend;

// A sized allocation provides a stable C userdata pointer to the trait object.
// Callbacks can execute on mpv threads; never capture GTK objects directly.
struct UpdateCallback(Box<dyn Fn() + Send + Sync>);

use crate::error::{Error, Result};

pub struct FastRenderContext {
    render_context: *mut mpv_sys::mpv_render_context,
    update_callback: Option<Box<UpdateCallback>>,
    // Dropped AFTER the C render context, including when a widget owns the
    // last reference. Raw unsafe constructors leave this unset.
    owner: Option<Arc<MpvBackend>>,
}

// Intentionally !Send / !Sync: rendering and destruction belong to the thread
// owning the current GL context. The raw C handle must not bypass GTK affinity.

impl FastRenderContext {
    /// # Safety
    /// `mpv` must be a valid, non-null mpv handle that outlives the returned context.
    pub unsafe fn new(mpv: *mut mpv_sys::mpv_handle) -> Result<Self> {
        unsafe { Self::new_with_wayland_display(mpv, None, false) }
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
        // Advanced control imposes an update-on-every-callback obligation on
        // the embedding application. It is not a shader/quality preset.
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
            // Enable only for an integration that services all render updates.
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
            update_callback: None,
            owner: None,
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
        // SAFETY: caller is on the owning render thread with its GL context
        // current. This is NOT callable from the mpv update callback.
        let flags = unsafe { mpv_sys::mpv_render_context_update(self.render_context) };
        (flags & u64::from(mpv_sys::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME)) != 0
    }

    /// Callback fires from an mpv internal thread — do NOT call mpv API from it.
    /// Schedule work on the main thread instead (e.g. `glib::idle_add_once`).
    pub fn set_update_callback<F: Fn() + Send + Sync + 'static>(&mut self, callback: F) {
        // Detach first: replacing the Rust allocation while mpv still has its
        // address can leave an in-flight callback dereferencing freed memory.
        unsafe {
            mpv_sys::mpv_render_context_set_update_callback(
                self.render_context,
                None,
                std::ptr::null_mut(),
            );
        }
        self.update_callback = Some(Box::new(UpdateCallback(Box::new(callback))));
        let userdata = self.update_callback.as_mut().expect("callback just installed");
        let userdata = std::ptr::from_mut(userdata.as_mut()).cast();
        // Registration can invoke the callback immediately. The allocation is
        // already owned by self and remains alive until unregistration.
        unsafe {
            mpv_sys::mpv_render_context_set_update_callback(
                self.render_context,
                Some(update_wrapper),
                userdata,
            );
        }
    }

    pub(super) fn with_owner(mut self, owner: Arc<MpvBackend>) -> Self {
        self.owner = Some(owner);
        self
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
        self.update_callback.take();
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
    update_callback: Option<Box<UpdateCallback>>,
    // Dropped AFTER the C render context, including when a widget owns the
    // last reference. Raw unsafe constructors leave this unset.
    owner: Option<Arc<MpvBackend>>,
}

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
            update_callback: None,
            owner: None,
        })
    }

    /// Render the current frame into `buffer` as `bgr0` pixels. `buffer` must be
    /// at least `stride * height` bytes. Returns 0 on success, negative on error.
    #[inline]
    pub fn render_into(&self, buffer: &mut [u8], width: i32, height: i32, stride: usize) -> i32 {
        if !super::frame_layout::buffer_fits(width, height, stride, buffer.len()) {
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
        // SAFETY: called on the owning render thread, never from the callback.
        let flags = unsafe { mpv_sys::mpv_render_context_update(self.render_context) };
        (flags & u64::from(mpv_sys::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME)) != 0
    }

    /// Callback fires from an mpv internal thread — schedule work on the main
    /// thread (e.g. `glib::idle_add_once`), never call mpv API from it.
    pub fn set_update_callback<F: Fn() + Send + Sync + 'static>(&mut self, callback: F) {
        // Detach first: replacing the Rust allocation while mpv still has its
        // address can leave an in-flight callback dereferencing freed memory.
        unsafe {
            mpv_sys::mpv_render_context_set_update_callback(
                self.render_context,
                None,
                std::ptr::null_mut(),
            );
        }
        self.update_callback = Some(Box::new(UpdateCallback(Box::new(callback))));
        let userdata = self.update_callback.as_mut().expect("callback just installed");
        let userdata = std::ptr::from_mut(userdata.as_mut()).cast();
        // Registration can invoke the callback immediately. The allocation is
        // already owned by self and remains alive until unregistration.
        unsafe {
            mpv_sys::mpv_render_context_set_update_callback(
                self.render_context,
                Some(update_wrapper),
                userdata,
            );
        }
    }

    pub(super) fn with_owner(mut self, owner: Arc<MpvBackend>) -> Self {
        self.owner = Some(owner);
        self
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
        self.update_callback.take();
        // SAFETY: render_context came from new() and is freed exactly once here.
        unsafe {
            mpv_sys::mpv_render_context_free(self.render_context);
        }
    }
}

/// SAFETY: userdata points to the sized allocation retained by its context.
/// The callback is detached before dropping that allocation.
unsafe extern "C" fn update_wrapper(userdata: *mut c_void) {
    // A panic may not cross the C ABI boundary (release uses panic=abort).
    let callback = unsafe { &*userdata.cast::<UpdateCallback>() };
    (callback.0)();
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
            2 * std::mem::size_of::<usize>(),
            "mpv_render_param is a C enum followed by an aligned pointer"
        );
        assert_eq!(
            std::mem::size_of::<mpv_sys::mpv_opengl_fbo>(),
            16,
            "mpv_opengl_fbo should be 16 bytes (4 x i32)"
        );
    }

    #[test]
    fn callback_can_be_shared_between_mpv_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UpdateCallback>();
    }
}
