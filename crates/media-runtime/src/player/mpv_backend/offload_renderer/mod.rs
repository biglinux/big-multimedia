//! GPU texture presentation through GtkGraphicsOffload. Offloading and direct
//! scanout depend on GTK, the compositor and the driver; no end-to-end zero-copy
//! or hardware-plane guarantee is implied. GL fallback remains available.

mod backend;
mod gl_fns;
mod slot_buffers;
mod stats;
mod texture_pool;

use super::egl_dmabuf::{self, DmaBufExport, EglDmaBufFns, FenceError};
use crate::player::mpv_backend::fast_render::{FastRenderContext, current_gl_driver};
use gtk::glib::translate::{ToGlibPtr, from_glib_full};
use gtk::{gdk, prelude::*};
use log::{debug, error, info, warn};
use std::cell::Cell;
use std::rc::Rc;
pub(super) use texture_pool::CurrentContext;
use texture_pool::{TextureLease, TexturePool};

use gl_fns::{
    GL_COLOR_ATTACHMENT0, GL_FRAMEBUFFER, GL_FRAMEBUFFER_COMPLETE, GL_LINEAR, GL_RGBA, GL_RGBA8,
    GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_TEXTURE_MIN_FILTER, GL_UNSIGNED_BYTE, GlFns,
};
use slot_buffers::SlotBuffers;
use stats::{RenderStatsSurfacePath, render_stats_report_for_interval};

pub use backend::VideoBackend;

/// A bounded three-slot GPU pool. GDK owns a lease on each published texture;
/// neither resize, overwrite nor deletion is permitted until release.
pub struct OffloadRenderer {
    gl_context: gdk::GLContext,
    gl: GlFns,
    picture: gtk::Picture,
    surface: gtk::Overlay,
    /// `Some` when MESA DMA-BUF export is available; path 1 is then tried first.
    egl: Option<EglDmaBufFns>,
    /// True after the first successful DMA-BUF export.
    dmabuf_active: Cell<bool>,
    /// True after an unsupported/non-linear modifier; locks the GL fallback.
    dmabuf_rejected: Cell<bool>,
    /// True after the kernel refused a dma-buf write fence; locks `glFinish`.
    fence_unsupported: Cell<bool>,
    /// The first frame whose fence could not be made has been reported.
    fence_failure_reported: Cell<bool>,
    pool: TexturePool,
    slots: SlotBuffers,
    fbo: u32,
    frame_count: Cell<u64>,
    skip_count: Cell<u64>,
    last_fps_log: Cell<std::time::Instant>,
}

impl OffloadRenderer {
    /// `None` when GL context creation fails or GL fns missing.
    pub fn try_new(display: &gdk::Display) -> Option<Self> {
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

        // SAFETY: `gl_context` was made current above.
        let diagnostics = unsafe { current_gl_driver() };
        if is_proprietary_nvidia(&diagnostics.vendor, &diagnostics.renderer) {
            info!(
                "Offload disabled for GL {} / {} because NVIDIA's proprietary \
                 GdkGLTexture/GraphicsOffload path grows GPU memory during \
                 resize; using GtkGLArea fallback",
                diagnostics.vendor, diagnostics.renderer
            );
            return None;
        }

        // Requires a current GL context.
        let egl = egl_dmabuf::probe();

        let pool = TexturePool::new(&gl_context, gl);
        let mut fbo = 0_u32;
        // SAFETY: `gl_context` is current and `fbo` is a valid out pointer.
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
            fence_unsupported: Cell::new(false),
            fence_failure_reported: Cell::new(false),
            pool,
            slots: SlotBuffers::default(),
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

    /// `true` when MESA DMA-BUF export is available. Drives the
    /// log-line distinction surfaced by `VideoBackend::detect`.
    pub(super) fn has_egl_dmabuf(&self) -> bool {
        self.egl.is_some()
    }

    /// Used by `init_render_context_offload` so mpv can resolve GL fns.
    pub fn make_context_current(&self) {
        self.gl_context.make_current();
    }

    /// Allocates at exact pixel size: rounding produces a black right/bottom
    /// margin in the GdkGLTexture path (NVIDIA): coords [0,1] map to the full
    /// backing texture, and GL textures don't support stride-based cropping
    /// the way DMA-BUF does.
    fn prepare_slot(&self, index: usize, texture: u32, width: i32, height: i32) -> bool {
        let changed = self.pool.dimensions(index) != (width, height);
        // Once GDK refused a dma-buf, frames go out as GL textures, which need
        // no buffer in the compositor's layout.
        let in_gbm_buffer = changed
            && !self.dmabuf_rejected.get()
            && self.slots.bind(
                index,
                texture,
                width,
                height,
                &self.picture,
                self.egl.as_ref(),
                &self.gl,
            );
        if changed {
            self.specify_texture(texture, width, height, !in_gbm_buffer);
        }
        let mut complete = self.attach_to_framebuffer(texture);
        if !complete && in_gbm_buffer {
            self.slots.disable(index);
            self.specify_texture(texture, width, height, true);
            complete = self.attach_to_framebuffer(texture);
        }
        if !complete {
            return false;
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
        // SAFETY: the context is current (`_current`) for these GL calls.
        unsafe { (self.gl.viewport)(0, 0, width, height) };
        let result = render_context.render(self.fbo as i32, width, height, false);
        // SAFETY: as above.
        unsafe { (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, 0) };
        if result < 0 {
            error!("Offload render failed: {result}");
            return;
        }
        let texture = match self.export_slot(index, tex) {
            Some(export) => {
                self.sync_frame(&export);
                match self.create_dmabuf_texture(export, lease, width, height) {
                    Ok(texture) => texture,
                    Err(lease) => self.create_gl_texture(lease, width, height),
                }
            }
            None => self.create_gl_texture(lease, width, height),
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

    /// Linear filtering, and storage from `glTexImage2D` when `allocate`; a
    /// texture bound to a GBM buffer already has its storage.
    fn specify_texture(&self, texture: u32, width: i32, height: i32, allocate: bool) {
        // SAFETY: the slot is exclusively leased and GL is current.
        unsafe {
            (self.gl.bind_texture)(GL_TEXTURE_2D, texture);
            (self.gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            (self.gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
            if allocate {
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
            }
            (self.gl.bind_texture)(GL_TEXTURE_2D, 0);
        }
    }

    /// Leaves the framebuffer bound when it is complete.
    fn attach_to_framebuffer(&self, texture: u32) -> bool {
        // SAFETY: the slot is exclusively leased and GL is current.
        unsafe {
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
        true
    }

    /// `None` once export failed or GDK refused a texture: the GL path takes over.
    fn export_slot(&self, index: usize, texture: u32) -> Option<DmaBufExport> {
        if self.dmabuf_rejected.get() {
            return None;
        }
        if let Some(export) = self.slots.export(index) {
            return Some(export);
        }
        let export = egl_dmabuf::export_gl_texture(self.egl.as_ref()?, texture);
        if export.is_none() {
            self.dmabuf_rejected.set(true);
        }
        export
    }

    /// Make readers of the exported frame wait for its rendering: through the
    /// dma-buf's write fence, or by `glFinish` where no fence can be attached.
    fn sync_frame(&self, export: &DmaBufExport) {
        if let Some(egl) = self.egl.as_ref()
            && let Some(fence) = egl.native_fence.as_ref()
            && !self.fence_unsupported.get()
        {
            match egl_dmabuf::attach_write_fence(egl, fence, self.gl.flush, export) {
                Ok(()) => return,
                Err(FenceError::Unsupported(error)) => {
                    warn!("DMA-BUF write fence refused ({error}); waiting with glFinish");
                    self.fence_unsupported.set(true);
                }
                Err(FenceError::Failed) => {
                    if !self.fence_failure_reported.replace(true) {
                        warn!(
                            "DMA-BUF write fence failed; this frame and any other that fails wait with glFinish"
                        );
                    }
                }
            }
        }
        // SAFETY: the context is current.
        unsafe { (self.gl.finish)() };
    }

    /// Wrap the export without releasing its lease until GDK finishes reading.
    fn create_dmabuf_texture(
        &self,
        export: DmaBufExport,
        lease: TextureLease,
        width: i32,
        height: i32,
    ) -> std::result::Result<gdk::Texture, TextureLease> {
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
                .set_fourcc(opaque_fourcc(export.fourcc))
                .set_modifier(export.modifier)
                .set_n_planes(1)
                .set_fd(0, export.fd)
                .set_stride(0, export.stride)
                .set_offset(0, export.offset)
                .set_premultiplied(false)
        };
        type Payload = (egl_dmabuf::DmaBufExport, TextureLease);
        unsafe extern "C" fn release(data: *mut std::ffi::c_void) {
            // SAFETY: GDK calls this once, only for a texture it created, with
            // the payload pointer handed to the builder below.
            drop(unsafe { Box::from_raw(data.cast::<Payload>()) });
        }
        let payload = Box::into_raw(Box::new((export, lease)));
        let mut error = std::ptr::null_mut();
        // Direct FFI reclaims the release userdata on build failure; the gtk-rs
        // convenience wrapper of the locked version leaks it.
        // SAFETY: the builder is alive for the call and `payload` is a leaked
        // Box that either `release` or the failure path below reclaims.
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
                // SAFETY: on failure GDK transfers a GError to the caller.
                let error: gtk::glib::Error = unsafe { from_glib_full(error) };
                warn!("DMA-BUF import rejected; using GL textures: {error}");
            }
            // SAFETY: GDK never calls `release` when the build fails.
            let (export, lease) = *unsafe { Box::from_raw(payload) };
            drop(export); // close the fd; the lease is reused by GL fallback
            return Err(lease);
        }
        self.dmabuf_active.set(true);
        // SAFETY: a non-null texture is a new reference owned by the caller.
        Ok(unsafe { from_glib_full(texture) })
    }

    fn create_gl_texture(&self, lease: TextureLease, width: i32, height: i32) -> gdk::Texture {
        const GL_SYNC_GPU_COMMANDS_COMPLETE: u32 = 0x9117;
        let fence = if let (Some(create), Some(_)) = (self.gl.fence_sync, self.gl.delete_sync) {
            // SAFETY: the context is current.
            unsafe { create(GL_SYNC_GPU_COMMANDS_COMPLETE, 0) }
        } else {
            std::ptr::null_mut()
        };
        // A shared GL context is not, by itself, a producer/consumer fence.
        // SAFETY: the context is current.
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
        // SAFETY: the lease keeps the texture id and its fence alive until GDK
        // releases the texture.
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

const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");
const DRM_FORMAT_XBGR8888: u32 = u32::from_le_bytes(*b"XB24");
const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");

/// The same layout with the alpha byte declared unused. mpv renders the frame
/// opaque, and GTK refuses to offload straight alpha the compositor cannot blend.
fn opaque_fourcc(fourcc: u32) -> u32 {
    match fourcc {
        DRM_FORMAT_ABGR8888 => DRM_FORMAT_XBGR8888,
        DRM_FORMAT_ARGB8888 => DRM_FORMAT_XRGB8888,
        other => other,
    }
}

/// Whether GL runs on NVIDIA's proprietary driver, directly or under zink, from
/// the context's `GL_VENDOR` and `GL_RENDERER`. An NVIDIA GPU on Mesa (NVK) is not.
fn is_proprietary_nvidia(vendor: &str, renderer: &str) -> bool {
    vendor.starts_with("NVIDIA") || renderer.contains("(NVIDIA_PROPRIETARY)")
}

impl Drop for OffloadRenderer {
    fn drop(&mut self) {
        let _current = CurrentContext::enter(&self.gl_context);
        self.picture.set_paintable(None::<&gdk::Texture>);
        // Slot textures and fences retire only after the final GDK lease. The
        // framebuffer has no consumer and can be deleted on this thread now.
        // SAFETY: the context is current and `fbo` was generated in it.
        unsafe { (self.gl.delete_framebuffers)(1, &raw const self.fbo) };
        info!("Offload renderer destroyed");
    }
}

#[cfg(test)]
mod tests {
    /// Expected codes are the `fourcc_code` values of `drm_fourcc.h`.
    #[test]
    fn alpha_formats_become_opaque() {
        for (exported, declared) in [
            (0x3432_4241, 0x3432_4258), // ABGR8888 -> XBGR8888
            (0x3432_5241, 0x3432_5258), // ARGB8888 -> XRGB8888
            (0x3432_4258, 0x3432_4258), // XBGR8888
            (0x3432_5258, 0x3432_5258), // XRGB8888
            (0x3231_564E, 0x3231_564E), // NV12
            (0, 0),
        ] {
            assert_eq!(
                super::opaque_fourcc(exported),
                declared,
                "exported 0x{exported:08X}"
            );
        }
    }

    /// Strings from `eglinfo -B` and the player's log on each driver; zink
    /// names its Vulkan driver by `VkDriverId`.
    #[test]
    fn proprietary_nvidia_from_gl_strings() {
        for (vendor, renderer, proprietary) in [
            (
                "NVIDIA Corporation",
                "NVIDIA GeForce GTX 1660 Ti/PCIe/SSE2",
                true,
            ),
            (
                "Mesa",
                "zink Vulkan 1.4(NVIDIA GeForce GTX 1660 Ti (NVIDIA_PROPRIETARY))",
                true,
            ),
            (
                "Mesa",
                "zink Vulkan 1.4(NVIDIA GeForce GTX 1660 Ti (MESA_NVK))",
                false,
            ),
            ("Intel", "Mesa Intel(R) HD Graphics 630 (KBL GT2)", false),
            ("Intel", "Mesa Intel(R) Graphics (ARL)", false),
            (
                "AMD",
                "AMD Radeon Graphics (radeonsi, renoir, ACO, DRM 3.61)",
                false,
            ),
            ("Mesa", "llvmpipe (LLVM 23.1.4, 256 bits)", false),
            ("", "", false),
        ] {
            assert_eq!(
                super::is_proprietary_nvidia(vendor, renderer),
                proprietary,
                "{vendor} / {renderer}"
            );
        }
    }
}
