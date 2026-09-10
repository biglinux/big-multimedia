//! Offload-based video rendering: bypasses GTK compositing via
//! GdkDmabufTexture or GdkGLTexture + GtkGraphicsOffload → Wayland subsurface.
//!
//! Path 1 — MESA DMA-BUF (AMD/Intel, preferred): mpv → FBO → EGL MESA export
//! → GdkDmabufTexture → GraphicsOffload → Wayland hw plane (zero GSK).
//! Path 2 — GL texture fallback: mpv → FBO → glFlush → GdkGLTexture →
//! GtkPicture → GraphicsOffload → Wayland subsurface.
//!
//! Path 1 is tried first. Proprietary NVIDIA is blocked before offload setup:
//! its GdkGLTexture/GraphicsOffload path grows GPU memory while resizing, so
//! the shared backend always uses GtkGLArea there.
//!
//! Split into submodules: `gl_fns` (GL constants + dlsym'd function
//! pointers), `backend` (public `VideoBackend` enum + detect). The
//! `OffloadRenderer` body stays here to keep the FBO/texture lifecycle
//! local to one file. Public API preserved.

mod backend;
mod gl_fns;
mod stats;

use super::egl_dmabuf::{self, EglDmaBufFns};
use crate::player::mpv_backend::fast_render::FastRenderContext;
use gtk::{gdk, prelude::*};
use log::{debug, error, info, warn};
use std::cell::Cell;
use std::ffi::CStr;

use super::skip_mpv_render;
use gl_fns::{
    GL_COLOR_ATTACHMENT0, GL_FRAMEBUFFER, GL_FRAMEBUFFER_COMPLETE, GL_LINEAR, GL_RGBA, GL_RGBA8,
    GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_TEXTURE_MIN_FILTER, GL_UNSIGNED_BYTE, GlFns,
};
use stats::{RenderStatsSurfacePath, render_stats_report_for_interval};

pub use backend::VideoBackend;

/// Double-buffered GL textures: while the compositor reads one (via DMA-BUF
/// or GdkGLTexture), mpv renders the other. Wrapped as `GdkDmabufTexture`
/// (path 1, AMD/Intel) or `GdkGLTexture` (path 2 fallback) and
/// displayed via `GtkPicture` inside `GtkGraphicsOffload`.
pub struct OffloadRenderer {
    gl_context: gdk::GLContext,
    gl: GlFns,
    picture: gtk::Picture,
    offload: gtk::GraphicsOffload,
    /// `Some` when MESA DMA-BUF export is available — path 1 is then tried first.
    egl: Option<EglDmaBufFns>,
    /// True after the first successful DMA-BUF export.
    dmabuf_active: Cell<bool>,
    /// True after an unsupported/non-linear modifier — locks the GL fallback.
    dmabuf_rejected: Cell<bool>,
    textures: [u32; 2],
    /// Re-attached to the current back-buffer texture each frame.
    fbo: u32,
    /// 0 or 1 — back-buffer index for the next render.
    current: Cell<usize>,
    /// `0` until the first allocation.
    width: Cell<i32>,
    height: Cell<i32>,
    /// Bypasses `has_new_frame()` once — used after window resize.
    force_render: Cell<bool>,
    frame_count: Cell<u64>,
    skip_count: Cell<u64>,
    last_fps_log: Cell<std::time::Instant>,
}

impl OffloadRenderer {
    /// `None` when GL context creation fails or GL fns missing.
    pub fn try_new(display: &gdk::Display) -> Option<Self> {
        if super::is_proprietary_nvidia_session() {
            info!("Offload disabled before GL setup because the session uses the NVIDIA driver");
            return None;
        }

        let gl_context = display
            .create_gl_context()
            .map_err(|e| {
                warn!("Offload: cannot create GL context: {e}");
            })
            .ok()?;

        gl_context
            .realize()
            .map_err(|e| {
                warn!("Offload: cannot realize GL context: {e}");
            })
            .ok()?;

        let gl = GlFns::load().or_else(|| {
            warn!("Offload: missing GL functions");
            None
        })?;

        let picture = gtk::Picture::new();
        picture.set_hexpand(true);
        picture.set_vexpand(true);
        picture.set_content_fit(gtk::ContentFit::Fill);
        picture.set_can_target(false);

        let offload = gtk::GraphicsOffload::new(Some(&picture));
        offload.set_enabled(gtk::GraphicsOffloadEnabled::Enabled);
        offload.set_hexpand(true);
        offload.set_vexpand(true);
        offload.set_black_background(true);
        offload.set_can_target(false);

        gl_context.make_current();

        let diagnostics = query_gl_diagnostics();
        if renderer_has_nvidia_resize_leak(&diagnostics.renderer) {
            info!(
                "Offload disabled for GL renderer '{}' because NVIDIA's \
                 GdkGLTexture/GraphicsOffload path grows GPU memory during \
                 resize; using GtkGLArea fallback",
                diagnostics.renderer
            );
            gdk::GLContext::clear_current();
            return None;
        }

        // Requires a current GL context.
        let egl = egl_dmabuf::probe();

        let mut textures = [0_u32; 2];
        let mut fbo = 0_u32;

        // SAFETY: GL fns verified non-null in load(); context current.
        unsafe {
            (gl.gen_textures)(2, textures.as_mut_ptr());
            (gl.gen_framebuffers)(1, &raw mut fbo);
        }

        gdk::GLContext::clear_current();

        let dmabuf_label = if egl.is_some() {
            "DMA-BUF (probe ok, modifier check on first frame)"
        } else {
            "GL texture"
        };
        info!(
            "Offload renderer ready: textures [{}, {}], FBO {fbo}, path: {dmabuf_label}",
            textures[0], textures[1]
        );

        // Compute shader compat diagnostic (GLES 3.1+ / GL 4.3+).
        if gl_context.is_legacy() {
            warn!("GL context is legacy (no compute shader support)");
        }
        if gl_context.api() == gdk::GLAPI::GLES {
            info!("GL context API: GLES (compute requires GLES 3.1+)");
        } else {
            info!("GL context API: OpenGL (compute requires GL 4.3+)");
        }

        info!(
            "GL version: {}, renderer: {}",
            diagnostics.version, diagnostics.renderer
        );
        gdk::GLContext::clear_current();

        Some(Self {
            gl_context,
            gl,
            picture,
            offload,
            egl,
            dmabuf_active: Cell::new(false),
            dmabuf_rejected: Cell::new(false),
            textures,
            fbo,
            current: Cell::new(0),
            width: Cell::new(0),
            height: Cell::new(0),
            force_render: Cell::new(false),
            frame_count: Cell::new(0),
            skip_count: Cell::new(0),
            last_fps_log: Cell::new(std::time::Instant::now()),
        })
    }

    pub fn widget(&self) -> &gtk::GraphicsOffload {
        &self.offload
    }

    /// `true` when MESA DMA-BUF export is available — drives the
    /// log-line distinction surfaced by `VideoBackend::detect`.
    pub(super) fn has_egl_dmabuf(&self) -> bool {
        self.egl.is_some()
    }

    /// Used by `init_render_context_offload` so mpv can resolve GL fns.
    pub fn make_context_current(&self) {
        self.gl_context.make_current();
    }

    /// Allocates at exact pixel size — rounding produces a black right/bottom
    /// margin in the GdkGLTexture path (NVIDIA): coords [0,1] map to the full
    /// backing texture, and GL textures don't support stride-based cropping
    /// the way DMA-BUF does.
    fn ensure_size(&self, width: i32, height: i32) {
        let cur_w = self.width.get();
        let cur_h = self.height.get();

        if cur_w == width && cur_h == height {
            return;
        }

        let gl = &self.gl;

        // SAFETY: own texture objects; positive dims (caller-checked).
        unsafe {
            for &tex in &self.textures {
                (gl.bind_texture)(GL_TEXTURE_2D, tex);
                (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
                (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
                (gl.tex_image_2d)(
                    GL_TEXTURE_2D,
                    0,
                    GL_RGBA8 as i32,
                    width,
                    height,
                    0,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    std::ptr::null(),
                );
            }
            (gl.bind_texture)(GL_TEXTURE_2D, 0);
        }

        // SAFETY: own FBO/texture.
        unsafe {
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (gl.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                self.textures[0],
                0,
            );
            let status = (gl.check_framebuffer_status)(GL_FRAMEBUFFER);
            if status != GL_FRAMEBUFFER_COMPLETE {
                error!("Offload FBO incomplete: 0x{status:X}");
            }
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, 0);
        }

        self.width.set(width);
        self.height.set(height);

        debug!("Offload FBO resized to {width}x{height}");
    }

    /// Called from the main thread (via `glib::idle_add_once`). Renders into
    /// the back-buffer, then exports as DMA-BUF or wraps as GdkGLTexture.
    pub fn render_frame(&self, render_context: &FastRenderContext, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }

        if skip_mpv_render() {
            render_context.report_swap();
            return;
        }

        // force_render bypasses has_new_frame for resize while paused.
        let force = self.force_render.replace(false);
        if !force && !render_context.has_new_frame() {
            self.skip_count.set(self.skip_count.get() + 1);
            return;
        }

        let count = self.frame_count.get() + 1;
        self.frame_count.set(count);
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_fps_log.get());
        let surface_path = RenderStatsSurfacePath::from_dmabuf_active(self.dmabuf_active.get());
        if let Some(render_stats_report) = render_stats_report_for_interval(
            count,
            self.skip_count.get(),
            elapsed,
            width,
            height,
            surface_path,
        ) {
            info!(
                "Render stats: {:.1} fps, {} rendered, {} skipped, {}x{} ({})",
                render_stats_report.frames_per_second,
                render_stats_report.rendered_frames,
                render_stats_report.skipped_frames,
                render_stats_report.width,
                render_stats_report.height,
                render_stats_report.surface_path.label()
            );
            self.frame_count.set(0);
            self.skip_count.set(0);
            self.last_fps_log.set(now);
        }

        self.gl_context.make_current();
        self.ensure_size(width, height);

        let idx = self.current.get();
        let tex = self.textures[idx];

        // SAFETY: own FBO/texture; valid dims.
        unsafe {
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (self.gl.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                tex,
                0,
            );
            (self.gl.viewport)(0, 0, width, height);
        }

        // flip_y=false keeps GL convention (Y=0 at bottom) — GDK flips on display.
        render_context.render(self.fbo as i32, width, height, false);
        render_context.report_swap();

        // DMA-BUF needs glFinish (compositor reads immediately — in-flight work
        // tears); GL-texture path only needs glFlush since GDK handles sync via
        // the shared context.
        // SAFETY: GL fns valid; pipeline sync per spec.
        unsafe {
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, 0);
            if self.egl.is_some() && !self.dmabuf_rejected.get() {
                (self.gl.finish)();
            } else {
                (self.gl.flush)();
            }
        }

        self.current.set(1 - idx);

        let texture = if self.dmabuf_rejected.get() {
            self.create_gl_texture(tex, width, height)
        } else {
            self.try_create_dmabuf_texture(tex, width, height)
                .unwrap_or_else(|| self.create_gl_texture(tex, width, height))
        };

        self.picture.set_paintable(Some(&texture));

        gdk::GLContext::clear_current();
    }

    /// Zero-copy path: GPU → compositor hw overlay via GraphicsOffload.
    /// `None` when EGL export is unavailable or has been rejected.
    fn try_create_dmabuf_texture(
        &self,
        gl_texture_id: u32,
        width: i32,
        height: i32,
    ) -> Option<gdk::Texture> {
        if self.dmabuf_rejected.get() {
            return None;
        }

        let egl = self.egl.as_ref()?;

        let Some(export) = egl_dmabuf::export_gl_texture(egl, gl_texture_id) else {
            // Modifier rejected or driver fail — lock in the GL fallback.
            self.dmabuf_rejected.set(true);
            return None;
        };

        let Some(display) = gdk::prelude::DrawContextExt::display(&self.gl_context) else {
            warn!("DMA-BUF: no display on GL context");
            return None;
        };

        let fd = export.fd;
        let fourcc = export.fourcc;
        let modifier = export.modifier;
        let stride = export.stride;
        let offset = export.offset;

        // Ownership of fd transferred to GDK via the release closure.
        std::mem::forget(export);

        // SAFETY: fd is a valid EGL-exported DMA-BUF; release closes it on finalize.
        let result = unsafe {
            gdk::DmabufTextureBuilder::new()
                .set_display(&display)
                .set_width(width as u32)
                .set_height(height as u32)
                .set_fourcc(fourcc)
                .set_modifier(modifier)
                .set_n_planes(1)
                .set_fd(0, fd)
                .set_stride(0, stride)
                .set_offset(0, offset)
                .set_premultiplied(false)
                .build_with_release_func(move || {
                    libc::close(fd);
                })
        };

        match result {
            Ok(texture) => {
                if !self.dmabuf_active.get() {
                    info!(
                        "DMA-BUF path activated: fourcc=0x{fourcc:08X}, \
                         modifier=0x{modifier:016X}, {width}x{height}"
                    );
                    self.dmabuf_active.set(true);
                }
                Some(texture)
            }
            Err(e) => {
                // Warn once, then fall back to GL silently.
                if self.dmabuf_active.get() || self.frame_count.get() <= 1 {
                    warn!("DMA-BUF texture failed, falling back to GL: {e}");
                    self.dmabuf_active.set(false);
                }
                // mem::forget'd above — must close manually here.
                unsafe {
                    libc::close(fd);
                }
                None
            }
        }
    }

    /// Universal fallback — GSK still composites, but works everywhere.
    fn create_gl_texture(&self, gl_texture_id: u32, width: i32, height: i32) -> gdk::Texture {
        let builder = gdk::GLTextureBuilder::new()
            .set_context(Some(&self.gl_context))
            .set_id(gl_texture_id)
            .set_width(width)
            .set_height(height)
            .set_format(gdk::MemoryFormat::R8g8b8a8);
        // SAFETY: GL texture valid; context current. No-op release keeps the
        // persistent texture alive after the GdkGLTexture drops.
        unsafe { builder.build_with_release_func(|| {}) }
    }

    /// Replaces GLArea's `queue_render` from the mpv update callback.
    pub fn request_render(
        &self,
        render_context: &FastRenderContext,
        overlay_width: i32,
        overlay_height: i32,
        scale: i32,
    ) {
        let w = overlay_width * scale;
        let h = overlay_height * scale;
        self.render_frame(render_context, w, h);
    }

    /// Bypasses `has_new_frame()` once — needed after resize while paused.
    pub fn set_force_render(&self) {
        self.force_render.set(true);
    }
}

struct GlDiagnostics {
    version: String,
    renderer: String,
}

fn query_gl_diagnostics() -> GlDiagnostics {
    type GlGetStringFn = unsafe extern "C" fn(u32) -> *const std::ffi::c_char;
    const GL_VERSION_ENUM: u32 = 0x1F02;
    const GL_RENDERER: u32 = 0x1F01;

    // SAFETY: dlsym RTLD_DEFAULT queries the process global symbol table.
    let get_string: Option<GlGetStringFn> = unsafe {
        let ptr = libc::dlsym(libc::RTLD_DEFAULT, c"glGetString".as_ptr());
        if ptr.is_null() {
            None
        } else {
            Some(std::mem::transmute::<*mut std::ffi::c_void, GlGetStringFn>(
                ptr,
            ))
        }
    };

    let Some(get_string) = get_string else {
        return GlDiagnostics {
            version: "<unavailable>".to_owned(),
            renderer: "<unavailable>".to_owned(),
        };
    };

    GlDiagnostics {
        version: gl_string(get_string, GL_VERSION_ENUM),
        renderer: gl_string(get_string, GL_RENDERER),
    }
}

fn gl_string(
    get_string: unsafe extern "C" fn(u32) -> *const std::ffi::c_char,
    name: u32,
) -> String {
    // SAFETY: caller has a current GL context; glGetString returns a static
    // nul-terminated string or null when unavailable.
    let ptr = unsafe { get_string(name) };
    if ptr.is_null() {
        return "<null>".to_owned();
    }
    // SAFETY: non-null pointer from GL for this context.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .unwrap_or("<invalid>")
        .to_owned()
}

fn renderer_has_nvidia_resize_leak(renderer: &str) -> bool {
    renderer.to_ascii_lowercase().contains("nvidia")
}

impl Drop for OffloadRenderer {
    fn drop(&mut self) {
        self.gl_context.make_current();

        // SAFETY: own GL objects from try_new.
        unsafe {
            (self.gl.delete_framebuffers)(1, &raw const self.fbo);
            (self.gl.delete_textures)(2, self.textures.as_ptr());
        }
        gdk::GLContext::clear_current();
        info!("Offload renderer destroyed");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn nvidia_renderer_is_rejected_for_offload() {
        assert!(super::renderer_has_nvidia_resize_leak(
            "NVIDIA GeForce RTX 3060/PCIe/SSE2"
        ));
        assert!(super::renderer_has_nvidia_resize_leak(
            "NVIDIA Corporation Ada Lovelace"
        ));
    }

    #[test]
    fn non_nvidia_renderers_keep_offload_eligible() {
        assert!(!super::renderer_has_nvidia_resize_leak(
            "AMD Radeon Graphics (RADV REMBRANDT)"
        ));
        assert!(!super::renderer_has_nvidia_resize_leak(
            "Mesa Intel(R) UHD Graphics"
        ));
        assert!(!super::renderer_has_nvidia_resize_leak("NVK on GSP"));
    }
}
