//! GL function pointers and constants used by the offload renderer.
//! Split out of `offload_renderer.rs`; behavior is identical.

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
}

impl GlFns {
    pub(super) fn load() -> Option<Self> {
        /// SAFETY: name must be null-terminated. dlsym with RTLD_DEFAULT queries
        /// the global symbol table; the returned pointer is null-checked.
        unsafe fn sym<F>(name: &[u8]) -> Option<F> {
            let ptr = unsafe {
                libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr().cast::<std::ffi::c_char>())
            };
            if ptr.is_null() {
                return None;
            }
            // F is always a bare fn pointer (pointer-width); guard against a
            // mistyped F that would make transmute_copy read out of bounds.
            debug_assert_eq!(
                std::mem::size_of::<F>(),
                std::mem::size_of::<*mut std::ffi::c_void>(),
                "GL symbol target F must be a plain function pointer",
            );
            // SAFETY: ptr is a non-null symbol address; F is a fn pointer of the
            // same width as the void* it was loaded as (asserted above).
            Some(unsafe { std::mem::transmute_copy::<*mut std::ffi::c_void, F>(&ptr) })
        }

        // SAFETY: names are null-terminated; GL extern "C" ABI is stable.
        unsafe {
            Some(Self {
                gen_textures: sym(b"glGenTextures\0")?,
                delete_textures: sym(b"glDeleteTextures\0")?,
                bind_texture: sym(b"glBindTexture\0")?,
                tex_parameteri: sym(b"glTexParameteri\0")?,
                tex_image_2d: sym(b"glTexImage2D\0")?,
                gen_framebuffers: sym(b"glGenFramebuffers\0")?,
                delete_framebuffers: sym(b"glDeleteFramebuffers\0")?,
                bind_framebuffer: sym(b"glBindFramebuffer\0")?,
                framebuffer_texture_2d: sym(b"glFramebufferTexture2D\0")?,
                check_framebuffer_status: sym(b"glCheckFramebufferStatus\0")?,
                viewport: sym(b"glViewport\0")?,
                flush: sym(b"glFlush\0")?,
                finish: sym(b"glFinish\0")?,
            })
        }
    }
}
