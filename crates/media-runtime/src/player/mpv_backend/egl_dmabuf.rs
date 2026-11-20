//! EGL DMA-BUF export: zero-copy GL texture → DMA-BUF for direct compositor
//! presentation via GdkDmabufTexture + GraphicsOffload.
//!
//! Pipeline: mpv → GL FBO → EglImageHandle → DMA-BUF fd → GdkDmabufTexture →
//! GraphicsOffload → wl_subsurface → compositor hw plane (zero GSK compositing).
//!
//! Uses `EGL_MESA_image_dma_buf_export`. Falls back to GdkGLTexture when the
//! extension is unavailable or export fails at runtime.

use super::gl_proc_address::{egl_function, gl_function};
use log::{debug, info, warn};
use std::ffi::{CStr, c_void};
use std::os::fd::RawFd;

type EglDisplayHandle = *mut c_void;
type EglContextHandle = *mut c_void;
type EglImageHandle = *mut c_void;
type EglInteger = i32;
type EglBooleanFlag = u32;

const EGL_TRUE: EglBooleanFlag = 1;
const EGL_NO_DISPLAY: EglDisplayHandle = std::ptr::null_mut();
const EGL_NO_CONTEXT: EglContextHandle = std::ptr::null_mut();
const EGL_NO_IMAGE: EglImageHandle = std::ptr::null_mut();
const EGL_GL_TEXTURE_2D: u32 = 0x30B1; // EGL_GL_TEXTURE_2D_KHR
const EGL_NONE: EglInteger = 0x3038;

type EglGetCurrentDisplayFn = unsafe extern "C" fn() -> EglDisplayHandle;
type EglGetCurrentContextFn = unsafe extern "C" fn() -> EglContextHandle;
type EglCreateImageFn = unsafe extern "C" fn(
    EglDisplayHandle,
    EglContextHandle,
    u32,
    *mut c_void,
    *const EglInteger,
) -> EglImageHandle;
type EglDestroyImageFn = unsafe extern "C" fn(EglDisplayHandle, EglImageHandle) -> EglBooleanFlag;
type EglExportQueryFn = unsafe extern "C" fn(
    EglDisplayHandle,
    EglImageHandle,
    *mut i32,
    *mut i32,
    *mut u64,
) -> EglBooleanFlag;
type EglExportImageFn = unsafe extern "C" fn(
    EglDisplayHandle,
    EglImageHandle,
    *mut i32,
    *mut EglInteger,
    *mut EglInteger,
) -> EglBooleanFlag;

pub(crate) struct EglDmaBufFns {
    get_display: EglGetCurrentDisplayFn,
    get_context: EglGetCurrentContextFn,
    create_image: EglCreateImageFn,
    destroy_image: EglDestroyImageFn,
    export_query: EglExportQueryFn,
    export_image: EglExportImageFn,
}

impl EglDmaBufFns {
    /// Core functions from libEGL, extension functions through `eglGetProcAddress`.
    pub(crate) fn load() -> Option<Self> {
        // SAFETY: each name is paired with its EGL signature.
        unsafe {
            let Some(get_display) = egl_function(c"eglGetCurrentDisplay") else {
                warn!("EGL DMA-BUF: EGL is not available");
                return None;
            };
            Some(Self {
                get_display,
                get_context: egl_function(c"eglGetCurrentContext")?,
                create_image: extension_function(c"eglCreateImageKHR")?,
                destroy_image: extension_function(c"eglDestroyImageKHR")?,
                export_query: extension_function(c"eglExportDMABUFImageQueryMESA")?,
                export_image: extension_function(c"eglExportDMABUFImageMESA")?,
            })
        }
    }
}

/// # Safety
/// `F` must be the `unsafe extern "C" fn` type of `name`.
unsafe fn extension_function<F: Copy>(name: &CStr) -> Option<F> {
    // SAFETY: forwarded from the caller.
    let function = unsafe { gl_function(name) };
    if function.is_none() {
        debug!("EGL DMA-BUF: missing extension function: {name:?}");
    }
    function
}

/// Owns the fd; `Drop` closes it.
pub(crate) struct DmaBufExport {
    pub fd: RawFd,
    pub fourcc: u32,
    pub modifier: u64,
    pub stride: u32,
    pub offset: u32,
}

impl Drop for DmaBufExport {
    fn drop(&mut self) {
        if self.fd >= 0 {
            // SAFETY: we own this fd (returned by EGL export).
            unsafe {
                libc::close(self.fd);
            }
            self.fd = -1;
        }
    }
}

/// Caller must have a current EGL context (`gdk::GLContext::make_current()`)
/// and `gl_texture_id` must be a valid `GL_TEXTURE_2D` name. The GL texture
/// is not affected when any EGL call fails (returns `None`).
pub(crate) fn export_gl_texture(egl: &EglDmaBufFns, gl_texture_id: u32) -> Option<DmaBufExport> {
    // SAFETY: EGL fn pointers verified non-null in load(); out-params stack-allocated.
    unsafe {
        let dpy = (egl.get_display)();
        if dpy == EGL_NO_DISPLAY {
            warn!("DMA-BUF export: no EGL display");
            return None;
        }

        let egl_context = (egl.get_context)();
        if egl_context == EGL_NO_CONTEXT {
            warn!("DMA-BUF export: no EGL context");
            return None;
        }

        let attribs = [EGL_NONE];
        let image = (egl.create_image)(
            dpy,
            egl_context,
            EGL_GL_TEXTURE_2D,
            gl_texture_id as usize as *mut c_void,
            attribs.as_ptr(),
        );
        if image == EGL_NO_IMAGE {
            warn!("DMA-BUF export: eglCreateImageKHR failed for texture {gl_texture_id}");
            return None;
        }

        let mut fourcc: i32 = 0;
        let mut num_planes: i32 = 0;
        // The query writes one modifier per plane (up to four), even before
        // we can reject a multi-plane image. A single u64 is not a valid array.
        let mut modifiers = [0_u64; 4];

        let query_ok = (egl.export_query)(
            dpy,
            image,
            &raw mut fourcc,
            &raw mut num_planes,
            modifiers.as_mut_ptr(),
        );
        if query_ok != EGL_TRUE || num_planes != 1 {
            warn!("DMA-BUF export: query failed (ok={query_ok}, planes={num_planes})");
            (egl.destroy_image)(dpy, image);
            return None;
        }

        // Any layout the driver exported is offered as-is, including tiled ones
        // and DRM_FORMAT_MOD_INVALID (the implicit layout AMD reports here):
        // GDK negotiates the format/modifier pair against the display when it
        // builds the texture and fails cleanly, and the caller then locks onto
        // the GL-texture path.
        let modifier = modifiers[0];

        // Exactly one plane was confirmed before handing these arrays to C.
        let mut fds = [-1_i32; 4];
        let mut strides = [0_i32; 4];
        let mut offsets = [0_i32; 4];

        let export_ok = (egl.export_image)(
            dpy,
            image,
            fds.as_mut_ptr(),
            strides.as_mut_ptr(),
            offsets.as_mut_ptr(),
        );

        // DMA-BUF fd keeps the buffer alive after image destruction.
        (egl.destroy_image)(dpy, image);

        if export_ok != EGL_TRUE || fds[0] < 0 || strides[0] <= 0 || offsets[0] < 0 {
            warn!("DMA-BUF export: eglExportDMABUFImageMESA failed");
            for &fd in &fds {
                if fd >= 0 {
                    libc::close(fd);
                }
            }
            return None;
        }

        // Single-plane RGBA: close any extra fds.
        for &fd in &fds[1..] {
            if fd >= 0 {
                libc::close(fd);
            }
        }

        debug!(
            "DMA-BUF exported: fd={}, fourcc=0x{:08X}, mod=0x{:016X}, stride={}, offset={}",
            fds[0], fourcc, modifier, strides[0], offsets[0]
        );

        Some(DmaBufExport {
            fd: fds[0],
            fourcc: fourcc as u32,
            modifier,
            stride: strides[0] as u32,
            offset: offsets[0] as u32,
        })
    }
}

/// Call with a current EGL/GL context.
pub(crate) fn probe() -> Option<EglDmaBufFns> {
    let fns = EglDmaBufFns::load();
    if fns.is_some() {
        info!("EGL DMA-BUF export: available (EGL_MESA_image_dma_buf_export)");
    } else {
        info!("EGL DMA-BUF export: not available (missing EGL functions)");
    }
    fns
}
