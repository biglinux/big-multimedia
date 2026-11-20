//! GL function pointers and constants used by the offload renderer.

use crate::player::mpv_backend::gl_proc_address::gl_function;

pub(super) const GL_TEXTURE_2D: u32 = 0x0DE1;
pub(super) const GL_RGBA: u32 = 0x1908;
pub(super) const GL_RGBA8: u32 = 0x8058;
pub(super) const GL_UNSIGNED_BYTE: u32 = 0x1401;
pub(super) const GL_FRAMEBUFFER: u32 = 0x8D40;
pub(super) const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
pub(super) const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
pub(super) const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
pub(super) const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
pub(super) const GL_LINEAR: i32 = 0x2601;

pub(super) type GlGenTexturesFn = unsafe extern "C" fn(i32, *mut u32);
pub(super) type GlDeleteTexturesFn = unsafe extern "C" fn(i32, *const u32);
pub(super) type GlBindTextureFn = unsafe extern "C" fn(u32, u32);
pub(super) type GlTexParameteriFn = unsafe extern "C" fn(u32, u32, i32);
pub(super) type GlTexImage2DFn =
    unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const std::ffi::c_void);
pub(super) type GlGenFramebuffersFn = unsafe extern "C" fn(i32, *mut u32);
pub(super) type GlDeleteFramebuffersFn = unsafe extern "C" fn(i32, *const u32);
pub(super) type GlBindFramebufferFn = unsafe extern "C" fn(u32, u32);
pub(super) type GlFramebufferTexture2DFn = unsafe extern "C" fn(u32, u32, u32, u32, i32);
pub(super) type GlCheckFramebufferStatusFn = unsafe extern "C" fn(u32) -> u32;
pub(super) type GlViewportFn = unsafe extern "C" fn(i32, i32, i32, i32);
pub(super) type GlFlushFn = unsafe extern "C" fn();
pub(super) type GlFinishFn = unsafe extern "C" fn();
pub(super) type GlGetErrorFn = unsafe extern "C" fn() -> u32;

#[derive(Clone, Copy)]
pub(super) struct GlFns {
    pub(super) gen_textures: GlGenTexturesFn,
    pub(super) delete_textures: GlDeleteTexturesFn,
    pub(super) bind_texture: GlBindTextureFn,
    pub(super) tex_parameteri: GlTexParameteriFn,
    pub(super) tex_image_2d: GlTexImage2DFn,
    pub(super) gen_framebuffers: GlGenFramebuffersFn,
    pub(super) delete_framebuffers: GlDeleteFramebuffersFn,
    pub(super) bind_framebuffer: GlBindFramebufferFn,
    pub(super) framebuffer_texture_2d: GlFramebufferTexture2DFn,
    pub(super) check_framebuffer_status: GlCheckFramebufferStatusFn,
    pub(super) viewport: GlViewportFn,
    pub(super) flush: GlFlushFn,
    pub(super) finish: GlFinishFn,
    pub(super) get_error: GlGetErrorFn,
    pub(super) fence_sync: Option<unsafe extern "C" fn(u32, u32) -> *mut std::ffi::c_void>,
    pub(super) delete_sync: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
}

impl GlFns {
    pub(super) fn load() -> Option<Self> {
        // SAFETY: each name is paired with its GL signature.
        unsafe {
            Some(Self {
                gen_textures: gl_function(c"glGenTextures")?,
                delete_textures: gl_function(c"glDeleteTextures")?,
                bind_texture: gl_function(c"glBindTexture")?,
                tex_parameteri: gl_function(c"glTexParameteri")?,
                tex_image_2d: gl_function(c"glTexImage2D")?,
                gen_framebuffers: gl_function(c"glGenFramebuffers")?,
                delete_framebuffers: gl_function(c"glDeleteFramebuffers")?,
                bind_framebuffer: gl_function(c"glBindFramebuffer")?,
                framebuffer_texture_2d: gl_function(c"glFramebufferTexture2D")?,
                check_framebuffer_status: gl_function(c"glCheckFramebufferStatus")?,
                viewport: gl_function(c"glViewport")?,
                flush: gl_function(c"glFlush")?,
                finish: gl_function(c"glFinish")?,
                get_error: gl_function(c"glGetError")?,
                fence_sync: gl_function(c"glFenceSync"),
                delete_sync: gl_function(c"glDeleteSync"),
            })
        }
    }
}
