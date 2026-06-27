//! `glGetIntegerv` access for the GLArea fallback render path.
//!
//! GTK4 GL contexts don't expose `glGetIntegerv`, so the currently bound
//! framebuffer (needed to blit mpv's frame into the GLArea) is read via a
//! scoped `dlopen("libGL.so.1", RTLD_LAZY|RTLD_LOCAL)` + `dlsym` (NOT
//! `RTLD_DEFAULT`, which would search every loaded object and is
//! `LD_PRELOAD`-hijackable) and cached thread-local.

/// `GL_FRAMEBUFFER_BINDING` enum value (queried to find the bound FBO).
pub const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;

/// ABI of `glGetIntegerv(GLenum pname, GLint *params)`.
pub type GlGetIntegervFn = unsafe extern "C" fn(u32, *mut i32);

/// Resolve (and cache) `glGetIntegerv` from `libGL.so.1`. Returns `None` if
/// the library or symbol is unavailable.
pub fn get_gl_get_integerv() -> Option<GlGetIntegervFn> {
    use std::cell::OnceCell;
    thread_local! {
        static CACHED: OnceCell<Option<GlGetIntegervFn>> = const { OnceCell::new() };
    }
    CACHED.with(|c| {
        *c.get_or_init(|| {
            let libname = c"libGL.so.1";
            // SAFETY: dlopen with `RTLD_LAZY | RTLD_LOCAL` returns a
            // refcounted handle scoped to libGL.so.1 only. The handle is
            // intentionally never `dlclose`d — its symbol resolution must
            // remain valid for the process lifetime (cached thread-local).
            // Scoping to libGL.so.1 (instead of `RTLD_DEFAULT`) prevents
            // LD_PRELOAD hijacks that could shadow glGetIntegerv from an
            // arbitrary preloaded object.
            let lib = unsafe { libc::dlopen(libname.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
            if lib.is_null() {
                return None;
            }
            let name = c"glGetIntegerv";
            // SAFETY: `lib` is a valid handle obtained from dlopen above.
            // dlsym returns NULL when the symbol is missing; we check below.
            let ptr = unsafe { libc::dlsym(lib, name.as_ptr()) };
            if ptr.is_null() {
                None
            } else {
                // SAFETY: ptr was verified non-null above. glGetIntegerv
                // has a known ABI (`extern "C" fn(u32, *mut i32)`)
                // guaranteed by the OpenGL specification. The transmute
                // matches `GlGetIntegervFn` exactly.
                Some(unsafe { std::mem::transmute::<*mut libc::c_void, GlGetIntegervFn>(ptr) })
            }
        })
    })
}
