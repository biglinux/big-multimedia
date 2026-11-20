//! EGL DMA-BUF export: zero-copy GL texture → DMA-BUF for direct compositor
//! presentation via GdkDmabufTexture + GraphicsOffload.
//!
//! Pipeline: mpv → GL FBO → EglImageHandle → DMA-BUF fd → GdkDmabufTexture →
//! GraphicsOffload → wl_subsurface → compositor hw plane (zero GSK compositing).
//!
//! Uses `EGL_MESA_image_dma_buf_export`. Falls back to GdkGLTexture when the
//! extension is unavailable or export fails at runtime. With
//! `EGL_ANDROID_native_fence_sync`, the GPU work that drew a frame becomes the
//! dma-buf's write fence, so readers wait for it instead of this thread.

use super::gl_proc_address::{egl_function, gl_function};
use log::{debug, info, warn};
use std::ffi::{CStr, c_char, c_void};
use std::os::fd::{FromRawFd, OwnedFd, RawFd};

type EglDisplayHandle = *mut c_void;
type EglContextHandle = *mut c_void;
type EglImageHandle = *mut c_void;
type EglSyncHandle = *mut c_void;
type EglInteger = i32;
type EglBooleanFlag = u32;

const EGL_TRUE: EglBooleanFlag = 1;
const EGL_NO_DISPLAY: EglDisplayHandle = std::ptr::null_mut();
const EGL_NO_CONTEXT: EglContextHandle = std::ptr::null_mut();
const EGL_NO_IMAGE: EglImageHandle = std::ptr::null_mut();
const EGL_GL_TEXTURE_2D: u32 = 0x30B1; // EGL_GL_TEXTURE_2D_KHR
const EGL_NONE: EglInteger = 0x3038;
const EGL_NO_SYNC: EglSyncHandle = std::ptr::null_mut();
const EGL_EXTENSIONS: EglInteger = 0x3055;
const EGL_SYNC_NATIVE_FENCE_ANDROID: u32 = 0x3144;
const EGL_NO_NATIVE_FENCE_FD_ANDROID: EglInteger = -1;
const NATIVE_FENCE_EXTENSION: &str = "EGL_ANDROID_native_fence_sync";
// linux/dma-buf.h: _IOW('b', 3, struct dma_buf_import_sync_file).
const DMA_BUF_IOCTL_IMPORT_SYNC_FILE: libc::Ioctl = 0x4008_6203;
const DMA_BUF_SYNC_WRITE: u32 = 2;

#[repr(C)]
struct DmaBufImportSyncFile {
    flags: u32,
    fd: i32,
}

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
type EglQueryStringFn = unsafe extern "C" fn(EglDisplayHandle, EglInteger) -> *const c_char;
type EglCreateSyncFn =
    unsafe extern "C" fn(EglDisplayHandle, u32, *const EglInteger) -> EglSyncHandle;
type EglDestroySyncFn = unsafe extern "C" fn(EglDisplayHandle, EglSyncHandle) -> EglBooleanFlag;
type EglDupNativeFenceFdFn = unsafe extern "C" fn(EglDisplayHandle, EglSyncHandle) -> EglInteger;

pub(crate) struct EglDmaBufFns {
    get_display: EglGetCurrentDisplayFn,
    get_context: EglGetCurrentContextFn,
    create_image: EglCreateImageFn,
    destroy_image: EglDestroyImageFn,
    export_query: EglExportQueryFn,
    export_image: EglExportImageFn,
    /// `None` when the display lacks `EGL_ANDROID_native_fence_sync`.
    pub(crate) native_fence: Option<NativeFenceFns>,
}

pub(crate) struct NativeFenceFns {
    create_sync: EglCreateSyncFn,
    destroy_sync: EglDestroySyncFn,
    dup_fence_fd: EglDupNativeFenceFdFn,
}

/// Why a frame could not carry its fence.
pub(crate) enum FenceError {
    /// This frame only; the next one tries again.
    Failed,
    /// The kernel has no `DMA_BUF_IOCTL_IMPORT_SYNC_FILE` for this buffer.
    Unsupported(std::io::Error),
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
                native_fence: load_native_fence(get_display),
            })
        }
    }
}

/// # Safety
/// Call with a current EGL context; `get_display` is `eglGetCurrentDisplay`.
unsafe fn load_native_fence(get_display: EglGetCurrentDisplayFn) -> Option<NativeFenceFns> {
    // SAFETY: each name is paired with its EGL signature; the display is the
    // current one and the returned string is owned by EGL.
    unsafe {
        let query_string: EglQueryStringFn = egl_function(c"eglQueryString")?;
        let extensions = query_string(get_display(), EGL_EXTENSIONS);
        if extensions.is_null()
            || !has_extension(
                &CStr::from_ptr(extensions).to_string_lossy(),
                NATIVE_FENCE_EXTENSION,
            )
        {
            return None;
        }
        Some(NativeFenceFns {
            create_sync: extension_function(c"eglCreateSyncKHR")?,
            destroy_sync: extension_function(c"eglDestroySyncKHR")?,
            dup_fence_fd: extension_function(c"eglDupNativeFenceFDANDROID")?,
        })
    }
}

/// `list` is EGL's space-separated extension string; a name matches only whole.
fn has_extension(list: &str, name: &str) -> bool {
    list.split_ascii_whitespace()
        .any(|extension| extension == name)
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

/// Make the GL work submitted so far the write fence of `export`'s dma-buf.
/// `flush` must be `glFlush` of the current context: the fence fd exists only
/// once the commands that signal it are flushed.
pub(crate) fn attach_write_fence(
    egl: &EglDmaBufFns,
    fence: &NativeFenceFns,
    flush: impl FnOnce(),
    export: &DmaBufExport,
) -> Result<(), FenceError> {
    // SAFETY: the context is current (caller), the functions were loaded for
    // its display, and the sync is destroyed on every path after creation.
    let sync_file = unsafe {
        let dpy = (egl.get_display)();
        let attribs = [EGL_NONE];
        let sync = (fence.create_sync)(dpy, EGL_SYNC_NATIVE_FENCE_ANDROID, attribs.as_ptr());
        if sync == EGL_NO_SYNC {
            return Err(FenceError::Failed);
        }
        flush();
        let fd = (fence.dup_fence_fd)(dpy, sync);
        (fence.destroy_sync)(dpy, sync);
        if fd == EGL_NO_NATIVE_FENCE_FD_ANDROID {
            return Err(FenceError::Failed);
        }
        // EGL hands over a new descriptor that nothing else closes.
        OwnedFd::from_raw_fd(fd)
    };
    let mut import = DmaBufImportSyncFile {
        flags: DMA_BUF_SYNC_WRITE,
        fd: std::os::fd::AsRawFd::as_raw_fd(&sync_file),
    };
    // SAFETY: `export.fd` is the open dma-buf and `import` matches the
    // kernel's struct dma_buf_import_sync_file for this request.
    let status = unsafe { libc::ioctl(export.fd, DMA_BUF_IOCTL_IMPORT_SYNC_FILE, &raw mut import) };
    if status == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ENOTTY | libc::EINVAL) => Err(FenceError::Unsupported(error)),
        _ => Err(FenceError::Failed),
    }
}

/// Call with a current EGL/GL context.
pub(crate) fn probe() -> Option<EglDmaBufFns> {
    let fns = EglDmaBufFns::load();
    // allow-noisy-log: one line per renderer, naming how frames are synchronized.
    match &fns {
        Some(fns) if fns.native_fence.is_some() => info!(
            "EGL DMA-BUF export: available (EGL_MESA_image_dma_buf_export), frames fenced by EGL_ANDROID_native_fence_sync"
        ),
        Some(_) => info!(
            "EGL DMA-BUF export: available (EGL_MESA_image_dma_buf_export), no native fence: glFinish per frame"
        ),
        None => info!("EGL DMA-BUF export: not available (missing EGL functions)"),
    }
    fns
}

#[cfg(test)]
mod tests {
    #[test]
    fn extension_matches_whole_token() {
        for (list, expected) in [
            (
                "EGL_KHR_fence_sync EGL_ANDROID_native_fence_sync EGL_KHR_wait_sync",
                true,
            ),
            ("EGL_ANDROID_native_fence_sync", true),
            ("  EGL_ANDROID_native_fence_sync\n", true),
            ("EGL_ANDROID_native_fence_sync_v2 EGL_KHR_fence_sync", false),
            ("XEGL_ANDROID_native_fence_sync", false),
            ("EGL_KHR_fence_sync", false),
            ("", false),
        ] {
            assert_eq!(
                super::has_extension(list, super::NATIVE_FENCE_EXTENSION),
                expected,
                "{list:?}"
            );
        }
    }
}
