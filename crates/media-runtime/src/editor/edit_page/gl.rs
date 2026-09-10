//! dlsym lookup of `glGetIntegerv` from `libGL.so.1` for FBO probing.

pub(super) const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
pub(super) type GlGetIntegervFn = unsafe extern "C" fn(u32, *mut i32);

#[must_use]
pub(super) fn gl_get_integerv() -> Option<GlGetIntegervFn> {
    use std::cell::OnceCell;
    use std::sync::OnceLock;

    // Resolve `libGL.so.1` directly instead of using `RTLD_DEFAULT` so a
    // hostile `LD_PRELOAD` cannot hijack `glGetIntegerv` and gain code
    // execution when the transmuted function pointer is invoked. The handle
    // is intentionally leaked (kept open for the process lifetime) so the
    // resolved symbol stays valid for every subsequent call.
    static LIBGL_HANDLE: OnceLock<usize> = OnceLock::new();

    thread_local! {
        static CACHED: OnceCell<Option<GlGetIntegervFn>> = const { OnceCell::new() };
    }

    CACHED.with(|cache| {
        *cache.get_or_init(|| {
            let handle_addr = *LIBGL_HANDLE.get_or_init(|| {
                let lib_name =
                    std::ffi::CString::new("libGL.so.1").expect("static string has no null bytes");
                // SAFETY: `lib_name` is a valid NUL-terminated C string; `dlopen`
                // is safe to call with any path and flag combination (returns null
                // on failure, checked below).
                let h =
                    unsafe { libc::dlopen(lib_name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
                h as usize
            });
            if handle_addr == 0 {
                return None;
            }
            let name =
                std::ffi::CString::new("glGetIntegerv").expect("static string has no null bytes");
            // SAFETY: `handle_addr` is a non-null handle from the `dlopen` above
            // (the `== 0` early-return guarantees it); `name` is NUL-terminated.
            let ptr = unsafe { libc::dlsym(handle_addr as *mut libc::c_void, name.as_ptr()) };
            if ptr.is_null() {
                None
            } else {
                // SAFETY: `glGetIntegerv` has the C ABI `extern "C" fn(u32, *mut i32)`
                // which is exactly `GlGetIntegervFn`; `ptr` is non-null (checked).
                Some(unsafe { std::mem::transmute::<*mut libc::c_void, GlGetIntegervFn>(ptr) })
            }
        })
    })
}
