//! Shared mpv video surface plumbing for player and converter editor.
//!
//! Keeps the main video path on mpv's render API + GraphicsOffload when
//! available, with GtkGLArea as fallback. Do not use CPU frame copies here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use adw::prelude::*;
use glib::translate::ToGlibPtr;
use gtk::{gdk, glib};
use log::{error, info};
use std::ffi::c_void;

use super::MpvBackend;
use super::fast_render::FastRenderContext;
use super::offload_renderer::VideoBackend;
use crate::error::{Error, Result};

pub fn wayland_display_ptr(display: &gdk::Display) -> Option<*mut c_void> {
    if !gdk::prelude::DisplayExtManual::backend(display).is_wayland() {
        return None;
    }

    // SAFETY: type name check above ensures the GdkDisplay is a Wayland display.
    let display_ptr: *mut gdk::ffi::GdkDisplay = display.to_glib_none().0;
    let ptr = unsafe { gdk_wayland_sys::gdk_wayland_display_get_wl_display(display_ptr.cast()) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr.cast())
    }
}

pub fn install_best_backend(
    display: &gdk::Display,
    overlay: &gtk::Overlay,
    gl_area: &gtk::GLArea,
    surface_name: &str,
) -> VideoBackend {
    let backend = VideoBackend::detect(display);

    if let VideoBackend::Offload(ref renderer) = backend {
        let offload_widget = renderer.widget().clone();
        offload_widget.set_hexpand(true);
        offload_widget.set_vexpand(true);

        // caveman: detect overlay shape. two callers:
        // (a) player → GLArea is an overlay child (add_overlay'd in .ui),
        //     overlay.child() is a different widget (the bg Box).
        // (b) editor → GLArea is the overlay's MAIN child (set_child).
        //     keeping a hidden GLArea as main child still triggers
        //     `Trying to snapshot GtkGLArea without a current allocation`
        //     warnings ~3-4/s because GtkOverlay snapshots its main child
        //     even when set_visible(false) — set_visible only suppresses
        //     map, not snapshot for the main child slot in GTK4.
        let gl_area_widget: &gtk::Widget = gl_area.upcast_ref();
        let gl_area_is_main_child = overlay.child().is_some_and(|c| &c == gl_area_widget);

        if gl_area_is_main_child {
            // caveman: editor path. Swap roles: offload becomes the main
            // child, GLArea gets unparented entirely. No snapshot of a
            // zero-allocation GLArea is possible because it's no longer in
            // the widget tree. The GLArea handle stays alive on the
            // EditPage struct (no leak, no double-free). On a second open
            // the EditPage is reused and video_backend is already Some, so
            // install_best_backend is not called again — the swap is a
            // one-shot operation per EditPage instance.
            overlay.set_child(Some(&offload_widget));
            info!(
                "{surface_name}: using offload backend (DMA-BUF/GraphicsOffload, swapped GLArea out)"
            );
        } else {
            // caveman: player path. Keep historical behavior — add_overlay
            // the offload widget right after the GLArea so it stacks above
            // the bg Box main child, and just hide the GLArea overlay child
            // (GTK4 honors set_visible(false) for overlay children).
            overlay.add_overlay(&offload_widget);
            offload_widget.insert_after(overlay, Some(gl_area));
            gl_area.set_visible(false);
            info!("{surface_name}: using offload backend (DMA-BUF/GraphicsOffload)");
        }
    } else {
        gl_area.set_halign(gtk::Align::Fill);
        gl_area.set_valign(gtk::Align::Fill);
        gl_area.set_hexpand(true);
        gl_area.set_vexpand(true);
        info!("{surface_name}: using GLArea backend (fallback)");
    }

    backend
}

pub fn create_render_context<F>(
    mpv: &std::sync::Arc<MpvBackend>,
    backend: Option<&VideoBackend>,
    gl_area: &gtk::GLArea,
    surface_name: &str,
    on_offload_frame: F,
) -> Result<FastRenderContext>
where
    F: Fn() + 'static,
{
    if let Some(VideoBackend::Offload(renderer)) = backend {
        renderer.make_context_current();
    } else {
        gl_area.make_current();
        if let Some(error) = gl_area.error() {
            let message = format!("{surface_name}: GLArea GL error: {error}");
            error!("{message}");
            return Err(Error::mpv(message));
        }
    }

    let wl_display = wayland_display_ptr(&gl_area.display());
    if wl_display.is_some() {
        info!("{surface_name}: passing Wayland display to libmpv render context");
    }
    // Advanced control only for the offload path (threaded render); the plain
    // GLArea path uses simple mode — advanced control deadlocks on software GL.
    let is_offload = matches!(backend, Some(VideoBackend::Offload(_)));
    let mut render_context =
        mpv.create_fast_render_context_with_wayland_display(wl_display, is_offload)?;
    if is_offload {
        install_main_thread_update_callback(&mut render_context, on_offload_frame);
    } else {
        install_glarea_update_callback(&mut render_context, gl_area);
    }
    info!("{surface_name}: render context initialized");
    Ok(render_context)
}

pub fn request_offload_render(
    backend: Option<&VideoBackend>,
    render_context: &FastRenderContext,
    surface: &impl IsA<gtk::Widget>,
) {
    let Some(VideoBackend::Offload(renderer)) = backend else {
        return;
    };

    renderer.request_render(
        render_context,
        surface.width(),
        surface.height(),
        surface.scale_factor(),
    );
}

pub fn render_glarea_frame(
    render_context: &FastRenderContext,
    framebuffer: i32,
    width: i32,
    height: i32,
    flip_y: bool,
) {
    if width <= 0 || height <= 0 {
        return;
    }

    render_context.render(framebuffer, width, height, flip_y);
    render_context.report_swap();
}

fn install_glarea_update_callback(render_context: &mut FastRenderContext, gl_area: &gtk::GLArea) {
    let render_pending = Arc::new(AtomicBool::new(false));
    let gl_send_weak = glib::SendWeakRef::from(gl_area.downgrade());
    let pending = render_pending.clone();

    render_context.set_update_callback(move || {
        if !pending.swap(true, Ordering::AcqRel) {
            let pending_inner = pending.clone();
            let gl_send_weak = gl_send_weak.clone();
            glib::idle_add_once(move || {
                pending_inner.store(false, Ordering::Release);
                if let Some(gl_area) = gl_send_weak.upgrade() {
                    gl_area.queue_render();
                }
            });
        }
    });
}

fn install_main_thread_update_callback<F>(render_context: &mut FastRenderContext, on_frame: F)
where
    F: Fn() + 'static,
{
    let (tx, rx) = async_channel::bounded::<()>(1);
    let render_pending = Arc::new(AtomicBool::new(false));
    let pending = render_pending.clone();
    render_context.set_update_callback(move || {
        if !pending.swap(true, Ordering::AcqRel) {
            let _ = tx.try_send(());
        }
    });

    glib::spawn_future_local(async move {
        while rx.recv().await.is_ok() {
            render_pending.store(false, Ordering::Release);
            on_frame();
        }
    });
}

#[cfg(test)]
mod tests {
    use big_mpv_render::{
        FORBIDDEN_MAIN_VIDEO_SURFACE_PATHS, MainVideoSurfacePath, PREFERRED_MAIN_VIDEO_BACKENDS,
    };

    #[test]
    fn video_surface_policy_keeps_main_frames_off_cpu() {
        assert!(
            FORBIDDEN_MAIN_VIDEO_SURFACE_PATHS
                .iter()
                .all(|surface_path| surface_path.is_cpu_copy_path()
                    && !surface_path.is_allowed_for_main_video_surface()),
            "main video must never use CPU-copy surfaces"
        );
        assert_eq!(
            PREFERRED_MAIN_VIDEO_BACKENDS,
            &[
                MainVideoSurfacePath::OffloadRenderer,
                MainVideoSurfacePath::GlAreaFallback
            ],
            "runtime must try offload before GLArea fallback"
        );
    }
}
