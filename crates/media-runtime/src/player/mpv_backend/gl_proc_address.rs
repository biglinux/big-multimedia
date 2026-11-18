//! EGL and GL entry points for every GL path of the players.
//!
//! GL functions come from `eglGetProcAddress`, as in mpv's own EGL backends.
//! Under libglvnd it returns the shared dispatch stubs for core and extension
//! functions alike, Mesa or NVIDIA, routed to whichever context GTK made
//! current, EGL or GLX. `dlsym(RTLD_DEFAULT)` finds them only while some loaded
//! object links libGL, and searches `LD_PRELOAD` objects first.

use std::ffi::{CStr, c_char, c_void};
use std::ptr::NonNull;
use std::sync::OnceLock;

type EglGetProcAddressFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;

struct Library(NonNull<c_void>);

// SAFETY: a dlopen handle names a process-wide object that is never closed,
// and dlsym may be called on it from any thread.
unsafe impl Send for Library {}
// SAFETY: as above; the handle is only read.
unsafe impl Sync for Library {}

/// `libEGL.so.1`, opened once and never closed so resolved functions stay valid.
#[expect(clippy::disallowed_methods, reason = "this module owns symbol lookup")]
fn egl_library() -> Option<&'static Library> {
    static LIBRARY: OnceLock<Option<Library>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            // SAFETY: a constant library name; dlopen returns the instance GTK
            // already loaded, or null.
            let handle = unsafe {
                libc::dlopen(c"libEGL.so.1".as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL)
            };
            NonNull::new(handle).map(Library)
        })
        .as_ref()
}

/// The EGL function `name` exported by `libEGL.so.1`.
///
/// # Safety
/// `F` must be the `unsafe extern "C" fn` type of `name`.
#[expect(clippy::disallowed_methods, reason = "this module owns symbol lookup")]
pub(crate) unsafe fn egl_function<F: Copy>(name: &CStr) -> Option<F> {
    let library = egl_library()?;
    // SAFETY: the handle is live for the process; `name` is NUL-terminated.
    let address = unsafe { libc::dlsym(library.0.as_ptr(), name.as_ptr()) };
    // SAFETY: the caller names `F` as the type of `name`.
    unsafe { function_at(address) }
}

/// Address of the GL function or EGL extension function `name`, null when no
/// EGL is installed.
pub fn gl_proc_address(name: &CStr) -> *mut c_void {
    static GET_PROC_ADDRESS: OnceLock<Option<EglGetProcAddressFn>> = OnceLock::new();
    // SAFETY: `eglGetProcAddress` has this signature in every EGL version.
    let resolved = *GET_PROC_ADDRESS.get_or_init(|| unsafe { egl_function(c"eglGetProcAddress") });
    match resolved {
        // SAFETY: `name` is NUL-terminated; EGL needs no current context here.
        Some(get_proc_address) => unsafe { get_proc_address(name.as_ptr()) },
        None => std::ptr::null_mut(),
    }
}

/// The GL function or EGL extension function `name`.
///
/// # Safety
/// `F` must be the `unsafe extern "C" fn` type of `name`.
pub unsafe fn gl_function<F: Copy>(name: &CStr) -> Option<F> {
    // SAFETY: the caller names `F` as the type of `name`.
    unsafe { function_at(gl_proc_address(name)) }
}

/// # Safety
/// `F` must be the function pointer type of the symbol at `address`.
unsafe fn function_at<F: Copy>(address: *mut c_void) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
    if address.is_null() {
        return None;
    }
    // SAFETY: non-null, and `F` is pointer-sized (checked at compile time) with
    // the signature the caller vouches for.
    Some(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&address) })
}
