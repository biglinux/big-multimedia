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

        // Two overlay shapes:
        // (a) player: GLArea is an overlay child (add_overlay'd in .ui),
        //     overlay.child() is a different widget (the bg Box).
        // (b) editor: GLArea is the overlay's main child (set_child).
        //     A hidden GLArea as main child still triggers repeated
        //     `Trying to snapshot GtkGLArea without a current allocation`
        //     warnings because GtkOverlay snapshots its main child even when
        //     set_visible(false): GTK4 only skips mapping that slot.
        let gl_area_widget: &gtk::Widget = gl_area.upcast_ref();
        let gl_area_is_main_child = overlay.child().is_some_and(|c| &c == gl_area_widget);

        if gl_area_is_main_child {
            // Editor path: offload becomes the main child and the GLArea is
            // unparented, so a zero-allocation GLArea is never snapshotted.
            // The GLArea handle stays alive on the EditPage struct. A second
            // open reuses the EditPage with video_backend already Some, so
            // the swap runs once per EditPage instance.
            overlay.set_child(Some(&offload_widget));
            info!(
                "{surface_name}: using offload backend (DMA-BUF/GraphicsOffload, swapped GLArea out)"
            );
        } else {
            // Player path: add_overlay the offload widget right after the
            // GLArea so it stacks above the bg Box main child, and hide the
            // GLArea overlay child (GTK4 honors set_visible(false) for overlay
            // children).
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
    // The render and property handlers share GTK's main thread. They cannot
    // promise advanced-control's no-core-wait contract. A dedicated render
    // actor is required before re-enabling direct-render advanced control.
    // Disabling it leaves libmpv's other no-cyclic-wait obligations in place.
    let context = gdk::GLContext::current()
        .ok_or_else(|| Error::mpv("No current GL context for the video surface"))?;
    // SAFETY: the owning GTK thread made this GL context current above. It
    // retains its display and is captured for exact-context teardown below.
    let mut render_context =
        unsafe { mpv.create_fast_render_context_with_wayland_display(wl_display, false)? };
    {
        render_context.set_context_scope(move |action| {
            struct Restore(Option<gdk::GLContext>);
            impl Drop for Restore {
                fn drop(&mut self) {
                    if let Some(previous) = &self.0 {
                        previous.make_current();
                    } else {
                        gdk::GLContext::clear_current();
                    }
                }
            }
            let _restore = Restore(gdk::GLContext::current());
            context.make_current();
            action();
        });
    }
    if let Some(VideoBackend::Offload(renderer)) = backend {
        let callback: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(on_offload_frame);
        renderer.set_retry_callback(callback.clone());
        install_main_thread_update_callback(&mut render_context, move || callback());
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
