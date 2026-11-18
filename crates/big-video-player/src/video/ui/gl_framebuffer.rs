//! `glGetIntegerv` access for the GLArea fallback render path.
//!
//! GTK4 GL contexts don't expose `glGetIntegerv`, so the currently bound
//! framebuffer (needed to blit mpv's frame into the GLArea) is read through the
//! function the player's GL loader resolves.

use std::sync::OnceLock;

use crate::player::mpv_backend::gl_proc_address::gl_function;

/// `GL_FRAMEBUFFER_BINDING` enum value (queried to find the bound FBO).
pub const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;

/// ABI of `glGetIntegerv(GLenum pname, GLint *params)`.
pub type GlGetIntegervFn = unsafe extern "C" fn(u32, *mut i32);

/// Resolve (and cache) `glGetIntegerv`. Returns `None` without EGL.
pub fn get_gl_get_integerv() -> Option<GlGetIntegervFn> {
    static CACHED: OnceLock<Option<GlGetIntegervFn>> = OnceLock::new();
    // SAFETY: `GlGetIntegervFn` is the signature of `glGetIntegerv`.
    *CACHED.get_or_init(|| unsafe { gl_function(c"glGetIntegerv") })
}
