//! EGL DMA-BUF export: zero-copy GL texture → DMA-BUF for direct compositor
//! presentation via GdkDmabufTexture + GraphicsOffload.
//!
//! Pipeline: mpv → GL FBO → EglImageHandle → DMA-BUF fd → GdkDmabufTexture →
//! GraphicsOffload → wl_subsurface → compositor hw plane (zero GSK compositing).
//!
//! Uses `EGL_MESA_image_dma_buf_export`. Falls back to GdkGLTexture when the
//! extension is unavailable or export fails at runtime.

use log::{debug, info, warn};
use std::ffi::c_void;
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

const DRM_FORMAT_MOD_LINEAR: u64 = 0;

/// Vendor-tiled layouts (NVIDIA block-linear, Intel X/Y-tiling) cause band
/// artifacts or compositor stalls because GDK's EGL import does not detile or
/// synchronize every layout correctly. Only linear is safe by default.
fn is_modifier_linear(modifier: u64) -> bool {
    modifier == DRM_FORMAT_MOD_LINEAR
}

type EglGetCurrentDisplayFn = unsafe extern "C" fn() -> EglDisplayHandle;
type EglGetCurrentContextFn = unsafe extern "C" fn() -> EglContextHandle;
type EglGetProcAddressFn = unsafe extern "C" fn(*const std::ffi::c_char) -> *mut c_void;
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
    /// Core fns via `dlsym`; extension fns via `eglGetProcAddress` (NVIDIA
    /// drivers don't expose extension entry points to plain dlsym).
    pub fn load() -> Option<Self> {
        // GTK loads libEGL internally but may not export symbols to RTLD_DEFAULT.
        // SAFETY: RTLD_NOLOAD first tries an existing handle, then falls back
        // to loading fresh.
        let lib = unsafe {
            let handle = libc::dlopen(c"libEGL.so.1".as_ptr(), libc::RTLD_LAZY | libc::RTLD_NOLOAD);
            if handle.is_null() {
                let handle =
                    libc::dlopen(c"libEGL.so.1".as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
                if handle.is_null() {
                    warn!("EGL DMA-BUF: cannot load libEGL.so.1");
                    return None;
                }
                handle
            } else {
                handle
            }
        };

        /// SAFETY: lib must be a valid dlopen handle, name null-terminated.
        unsafe fn dlsym_fn<F>(lib: *mut c_void, name: &[u8]) -> Option<F> {
            let ptr = unsafe { libc::dlsym(lib, name.as_ptr().cast::<std::ffi::c_char>()) };
            if ptr.is_null() {
                return None;
            }
            // F is always a bare fn pointer (pointer-width); guard against a
            // mistyped F that would make transmute_copy read out of bounds.
            debug_assert_eq!(
                std::mem::size_of::<F>(),
                std::mem::size_of::<*mut c_void>(),
                "dlsym target F must be a plain function pointer",
            );
            // SAFETY: ptr is a non-null symbol address; F is a fn pointer of the
            // same width as the void* it was loaded as (asserted above).
            Some(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&ptr) })
        }

        let get_display: EglGetCurrentDisplayFn =
            unsafe { dlsym_fn(lib, b"eglGetCurrentDisplay\0")? };
        let get_context: EglGetCurrentContextFn =
            unsafe { dlsym_fn(lib, b"eglGetCurrentContext\0")? };
        let get_proc_address: EglGetProcAddressFn =
            unsafe { dlsym_fn(lib, b"eglGetProcAddress\0")? };

        /// SAFETY: get_proc_address must be a valid eglGetProcAddress pointer.
        unsafe fn egl_fn<F>(get_proc_address: EglGetProcAddressFn, name: &[u8]) -> Option<F> {
            let ptr = unsafe { get_proc_address(name.as_ptr().cast::<std::ffi::c_char>()) };
            if ptr.is_null() {
                debug!(
                    "EGL DMA-BUF: missing extension function: {}",
                    std::str::from_utf8(&name[..name.len() - 1]).unwrap_or("?")
                );
                return None;
            }
            // F is always a bare fn pointer (pointer-width); guard against a
            // mistyped F that would make transmute_copy read out of bounds.
            debug_assert_eq!(
                std::mem::size_of::<F>(),
                std::mem::size_of::<*mut c_void>(),
                "eglGetProcAddress target F must be a plain function pointer",
            );
            // SAFETY: ptr is a non-null symbol address; F is a fn pointer of the
            // same width as the void* it was loaded as (asserted above).
            Some(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&ptr) })
        }

        // Intentionally not dlclose(lib) — functions must stay valid for the
        // process lifetime; GTK also keeps EGL loaded.
        // SAFETY: get_proc_address is valid; names are null-terminated.
        unsafe {
            Some(Self {
                get_display,
                get_context,
                create_image: egl_fn(get_proc_address, b"eglCreateImageKHR\0")?,
                destroy_image: egl_fn(get_proc_address, b"eglDestroyImageKHR\0")?,
                export_query: egl_fn(get_proc_address, b"eglExportDMABUFImageQueryMESA\0")?,
                export_image: egl_fn(get_proc_address, b"eglExportDMABUFImageMESA\0")?,
            })
        }
    }
}

/// Owns the fd — `Drop` closes it.
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
        let mut modifier: u64 = 0;

        let query_ok = (egl.export_query)(
            dpy,
            image,
            &raw mut fourcc,
            &raw mut num_planes,
            &raw mut modifier,
        );
        if query_ok != EGL_TRUE || num_planes < 1 {
            warn!("DMA-BUF export: query failed (ok={query_ok}, planes={num_planes})");
            (egl.destroy_image)(dpy, image);
            return None;
        }

        // Non-linear/implicit layouts are not safe for direct GDK DMA-BUF import.
        if !is_modifier_linear(modifier) {
            let vendor = (modifier >> 56) as u8;
            info!(
                "DMA-BUF export: non-linear modifier 0x{modifier:016X} \
                 (vendor=0x{vendor:02X}), falling back to GL texture"
            );
            (egl.destroy_image)(dpy, image);
            return None;
        }

        // Up to 4 planes — single-plane RGBA expected.
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

        if export_ok != EGL_TRUE || fds[0] < 0 {
            warn!("DMA-BUF export: eglExportDMABUFImageMESA failed");
            for &fd in &fds {
                if fd >= 0 {
                    libc::close(fd);
                }
            }
            return None;
        }

        // Single-plane RGBA — close any extra fds.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dmabuf_export_drop_closes_fd() {
        // -1 guard avoids close() on a sentinel fd.
        let export = DmaBufExport {
            fd: -1,
            fourcc: 0,
            modifier: 0,
            stride: 0,
            offset: 0,
        };
        drop(export);
    }

    #[test]
    fn egl_constants_match_spec() {
        assert_eq!(EGL_TRUE, 1);
        assert_eq!(EGL_GL_TEXTURE_2D, 0x30B1);
        assert_eq!(EGL_NONE, 0x3038);
    }

    #[test]
    fn only_linear_modifier_is_safe_by_default() {
        assert!(is_modifier_linear(DRM_FORMAT_MOD_LINEAR));
        assert!(!is_modifier_linear((1_u64 << 56) - 1));
        assert!(!is_modifier_linear(0x0100_0000_0000_0001));
    }

    #[test]
    fn egl_null_pointers_are_distinct() {
        assert!(EGL_NO_DISPLAY.is_null());
        assert!(EGL_NO_CONTEXT.is_null());
        assert!(EGL_NO_IMAGE.is_null());
    }

    #[test]
    fn probe_returns_none_without_egl() {
        // Without a real EGL display load() must return None — no panic.
        let _ = EglDmaBufFns::load();
    }
}
