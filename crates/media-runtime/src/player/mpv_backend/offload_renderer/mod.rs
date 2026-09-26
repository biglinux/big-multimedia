//! GPU texture presentation through GtkGraphicsOffload. Offloading and direct
//! scanout depend on GTK, the compositor and the driver; no end-to-end zero-copy
//! or hardware-plane guarantee is implied. GL fallback remains available.

mod backend;
mod gl_fns;
mod stats;
mod texture_pool;

use super::egl_dmabuf::{self, EglDmaBufFns};
use crate::player::mpv_backend::fast_render::FastRenderContext;
use gtk::glib::translate::{ToGlibPtr, from_glib_full};
use gtk::{gdk, prelude::*};
use log::{debug, error, info, warn};
use std::cell::Cell;
use std::ffi::CStr;
use std::rc::Rc;
use texture_pool::{CurrentContext, TextureLease, TexturePool};

use gl_fns::{
    GL_COLOR_ATTACHMENT0, GL_FRAMEBUFFER, GL_FRAMEBUFFER_COMPLETE, GL_LINEAR, GL_RGBA, GL_RGBA8,
    GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_TEXTURE_MIN_FILTER, GL_UNSIGNED_BYTE, GlFns,
};
use stats::{RenderStatsSurfacePath, render_stats_report_for_interval};

pub use backend::VideoBackend;

/// A bounded three-slot GPU pool. GDK owns a lease on each published texture;
/// neither resize, overwrite nor deletion is permitted until release.
pub struct OffloadRenderer {
    gl_context: gdk::GLContext,
    gl: GlFns,
    picture: gtk::Picture,
    surface: gtk::Overlay,
    /// `Some` when MESA DMA-BUF export is available — path 1 is then tried first.
    egl: Option<EglDmaBufFns>,
    /// True after the first successful DMA-BUF export.
    dmabuf_active: Cell<bool>,
    /// True after an unsupported/non-linear modifier — locks the GL fallback.
    dmabuf_rejected: Cell<bool>,
    pool: TexturePool,
    fbo: u32,
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
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_can_target(false);

        let offload = gtk::GraphicsOffload::new(Some(&picture));
        offload.set_enabled(gtk::GraphicsOffloadEnabled::Enabled);
        offload.set_hexpand(true);
        offload.set_vexpand(true);
        offload.set_black_background(true);
        offload.set_can_target(false);

        let _current = CurrentContext::enter(&gl_context);

        let diagnostics = query_gl_diagnostics();
        if renderer_has_nvidia_resize_leak(&diagnostics.renderer) {
            info!(
                "Offload disabled for GL renderer '{}' because NVIDIA's \
                 GdkGLTexture/GraphicsOffload path grows GPU memory during \
                 resize; using GtkGLArea fallback",
                diagnostics.renderer
            );
            return None;
        }

        // Requires a current GL context.
        let egl = egl_dmabuf::probe();

        let pool = TexturePool::new(&gl_context, gl);
        let mut fbo = 0_u32;
        unsafe { (gl.gen_framebuffers)(1, &raw mut fbo) };

        // Allocation notifications, not a permanent frame-clock/timer. The
        // transparent probe draws nothing; offload still directly owns Picture.
        let surface = gtk::Overlay::new();
        surface.set_hexpand(true);
        surface.set_vexpand(true);
        surface.set_can_target(false);
        surface.set_child(Some(&offload));
        let resize_probe = gtk::DrawingArea::new();
        resize_probe.set_can_target(false);
        resize_probe.set_opacity(0.0);
        resize_probe.set_hexpand(true);
        resize_probe.set_vexpand(true);
        surface.add_overlay(&resize_probe);
        pool.watch_resize(&resize_probe);

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

        Some(Self {
            gl_context,
            gl,
            picture,
            surface,
            egl,
            dmabuf_active: Cell::new(false),
            dmabuf_rejected: Cell::new(false),
            pool,
            fbo,
            frame_count: Cell::new(0),
            skip_count: Cell::new(0),
            last_fps_log: Cell::new(std::time::Instant::now()),
        })
    }

    pub fn widget(&self) -> &gtk::Overlay {
        &self.surface
    }

    /// The callback must use weak UI/backend references to avoid a cycle.
    pub fn set_retry_callback(&self, callback: Rc<dyn Fn()>) {
        self.pool.set_redraw(callback);
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
    fn prepare_slot(&self, index: usize, texture: u32, width: i32, height: i32) -> bool {
        let changed = self.pool.dimensions(index) != (width, height);
        // SAFETY: the slot is exclusively leased and GL is current.
        unsafe {
            if changed {
                (self.gl.bind_texture)(GL_TEXTURE_2D, texture);
                (self.gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
                (self.gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
                (self.gl.tex_image_2d)(
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
                (self.gl.bind_texture)(GL_TEXTURE_2D, 0);
            }
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (self.gl.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                texture,
                0,
            );
            let status = (self.gl.check_framebuffer_status)(GL_FRAMEBUFFER);
            if status != GL_FRAMEBUFFER_COMPLETE {
                (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, 0);
                error!("Offload FBO incomplete: 0x{status:X}");
                return false;
            }
        }
        if changed {
            self.pool.set_dimensions(index, width, height);
            debug!("Offload slot {index} resized to {width}x{height}");
        }
        true
    }

    /// Called from the main thread (via `glib::idle_add_once`). Renders into
    /// the back-buffer, then exports as DMA-BUF or wraps as GdkGLTexture.
    pub fn render_frame(&self, render_context: &FastRenderContext, width: i32, height: i32) {
        let _current = CurrentContext::enter(&self.gl_context);
        // update() must run with this exact context, including forced redraws.
        let new_frame = render_context.has_new_frame();
        let force = self.pool.take_force();
        let Some((width, height)) = super::frame_layout::bounded_surface(width, height) else {
            self.pool.force();
            return;
        };
        if !new_frame && !force {
            self.skip_count.set(self.skip_count.get() + 1);
            return;
        }
        let Some((index, lease)) = self.pool.acquire(&self.gl) else {
            self.skip_count.set(self.skip_count.get() + 1);
            return; // release callback retries the latest frame, never overwrites
        };
        let tex = lease.id();
        if !self.prepare_slot(index, tex, width, height) {
            return;
        }
        unsafe { (self.gl.viewport)(0, 0, width, height) };
        let result = render_context.render(self.fbo as i32, width, height, false);
        unsafe { (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, 0) };
        if result < 0 {
            error!("Offload render failed: {result}");
            return;
        }
        let texture = if self.egl.is_some() && !self.dmabuf_rejected.get() {
            // DMA-BUF explicit-fence negotiation is not implemented here. Keep
            // the correctness barrier; do NOT remove it on a performance guess.
            unsafe { (self.gl.finish)() };
            match self.try_create_dmabuf_texture(lease, width, height) {
                Ok(texture) => texture,
                Err(lease) => self.create_gl_texture(lease, width, height),
            }
        } else {
            self.create_gl_texture(lease, width, height)
        };
        self.picture.set_paintable(Some(&texture));

        let count = self.frame_count.get() + 1;
        self.frame_count.set(count);
        let now = std::time::Instant::now();
        if let Some(report) = render_stats_report_for_interval(
            count,
            self.skip_count.get(),
            now.duration_since(self.last_fps_log.get()),
            width,
            height,
            RenderStatsSurfacePath::from_dmabuf_active(self.dmabuf_active.get()),
        ) {
            info!(
                "Render stats: {:.1} fps, {} rendered, {} skipped, {}x{} ({})",
                report.frames_per_second,
                report.rendered_frames,
                report.skipped_frames,
                report.width,
                report.height,
                report.surface_path.label()
            );
            self.frame_count.set(0);
            self.skip_count.set(0);
            self.last_fps_log.set(now);
        }
    }

    /// Export the slot without releasing its lease until GDK finishes reading.
    fn try_create_dmabuf_texture(
        &self,
        lease: TextureLease,
        width: i32,
        height: i32,
    ) -> std::result::Result<gdk::Texture, TextureLease> {
        let Some(egl) = self.egl.as_ref() else {
            return Err(lease);
        };
        let Some(export) = egl_dmabuf::export_gl_texture(egl, lease.id()) else {
            self.dmabuf_rejected.set(true);
            return Err(lease);
        };
        let Some(display) = gdk::prelude::DrawContextExt::display(&self.gl_context) else {
            self.dmabuf_rejected.set(true);
            return Err(lease);
        };
        // SAFETY: `set_fd` requires the descriptor to stay open for as long as
        // the builder and the texture it produces may read it. `export` owns
        // the fd and moves into the release payload below, so it outlives both
        // the successful texture and the build-failure path that drops it.
        let builder = unsafe {
            gdk::DmabufTextureBuilder::new()
                .set_display(&display)
                .set_width(width as u32)
                .set_height(height as u32)
                .set_fourcc(export.fourcc)
                .set_modifier(export.modifier)
                .set_n_planes(1)
                .set_fd(0, export.fd)
                .set_stride(0, export.stride)
                .set_offset(0, export.offset)
                .set_premultiplied(false)
        };
        type Payload = (egl_dmabuf::DmaBufExport, TextureLease);
        unsafe extern "C" fn release(data: *mut std::ffi::c_void) {
            // Called only for a successfully-created texture, exactly once.
            drop(unsafe { Box::from_raw(data.cast::<Payload>()) });
        }
        let payload = Box::into_raw(Box::new((export, lease)));
        let mut error = std::ptr::null_mut();
        // Direct FFI deliberately reclaims release userdata on build failure.
        // The gtk-rs convenience wrapper in the reviewed version did not do so.
        let texture = unsafe {
            gdk::ffi::gdk_dmabuf_texture_builder_build(
                builder.to_glib_none().0,
                Some(release),
                payload.cast(),
                &raw mut error,
            )
        };
        if texture.is_null() {
            self.dmabuf_rejected.set(true); // do not retry/import/glFinish every frame
            self.dmabuf_active.set(false);
            if !error.is_null() {
                let error: gtk::glib::Error = unsafe { from_glib_full(error) };
                warn!("DMA-BUF import rejected; using GL textures: {error}");
            }
            let (export, lease) = *unsafe { Box::from_raw(payload) };
            drop(export); // close the fd; the lease is reused by GL fallback
            return Err(lease);
        }
        self.dmabuf_active.set(true);
        Ok(unsafe { from_glib_full(texture) })
    }

    fn create_gl_texture(&self, lease: TextureLease, width: i32, height: i32) -> gdk::Texture {
        const GL_SYNC_GPU_COMMANDS_COMPLETE: u32 = 0x9117;
        let fence = if let (Some(create), Some(_)) = (self.gl.fence_sync, self.gl.delete_sync) {
            unsafe { create(GL_SYNC_GPU_COMMANDS_COMPLETE, 0) }
        } else {
            std::ptr::null_mut()
        };
        // A shared GL context is not, by itself, a producer/consumer fence.
        unsafe {
            if fence.is_null() {
                (self.gl.finish)();
            } else {
                (self.gl.flush)();
            }
        }
        lease.set_fence(fence as usize);
        let builder = gdk::GLTextureBuilder::new()
            .set_context(Some(&self.gl_context))
            .set_id(lease.id())
            .set_width(width)
            .set_height(height)
            .set_format(gdk::MemoryFormat::R8g8b8a8)
            .set_sync((!fence.is_null()).then_some(fence.cast_const()));
        unsafe { builder.build_with_release_func(move || drop(lease)) }
    }

    /// Replaces GLArea's `queue_render` from the mpv update callback.
    pub fn request_render(
        &self,
        render_context: &FastRenderContext,
        overlay_width: i32,
        overlay_height: i32,
        scale: i32,
    ) {
        let Some(w) = overlay_width.checked_mul(scale) else {
            return;
        };
        let Some(h) = overlay_height.checked_mul(scale) else {
            return;
        };
        self.render_frame(render_context, w, h);
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
        let _current = CurrentContext::enter(&self.gl_context);
        self.picture.set_paintable(None::<&gdk::Texture>);
        // Slot textures and fences retire only after the final GDK lease. The
        // framebuffer has no consumer and can be deleted on this thread now.
        unsafe { (self.gl.delete_framebuffers)(1, &raw const self.fbo) };
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
