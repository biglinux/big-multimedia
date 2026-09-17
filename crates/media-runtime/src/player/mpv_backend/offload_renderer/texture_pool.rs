//! Bounded, release-driven texture ownership. GDK may retain several frames;
//! alternating texture IDs is NOT evidence that a buffer is safe to overwrite.

use super::gl_fns::GlFns;
use gtk::{gdk, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const SLOT_COUNT: usize = 3;
type RetiredTexture = (u32, usize);

/// Restore GTK's previous current context even on error/early return.
pub(super) struct CurrentContext(Option<gdk::GLContext>);
impl CurrentContext {
    pub(super) fn enter(context: &gdk::GLContext) -> Self {
        let previous = gdk::GLContext::current();
        context.make_current();
        Self(previous)
    }
}
impl Drop for CurrentContext {
    fn drop(&mut self) {
        if let Some(context) = &self.0 {
            context.make_current();
        } else {
            gdk::GLContext::clear_current();
        }
    }
}

#[derive(Default)]
struct Signals {
    pending: Cell<bool>,
    force: Cell<bool>,
    redraw: RefCell<Option<Rc<dyn Fn()>>>,
}

struct TextureResource {
    id: u32,
    busy: AtomicBool,
    fence: AtomicUsize,
    retired: async_channel::Sender<RetiredTexture>,
    wake: async_channel::Sender<()>,
}
impl Drop for TextureResource {
    fn drop(&mut self) {
        // Exactly SLOT_COUNT resources exist: at most three retirement messages.
        // The receiver owns the GL context and deletes on its original thread.
        let _ = self
            .retired
            .try_send((self.id, self.fence.load(Ordering::Acquire)));
    }
}

/// Send-safe release token. It contains no GTK object, and never calls GL from
/// GDK's potentially non-main release thread.
pub(super) struct TextureLease(Arc<TextureResource>);
impl TextureLease {
    pub(super) fn id(&self) -> u32 {
        self.0.id
    }
    pub(super) fn set_fence(&self, fence: usize) {
        self.0.fence.store(fence, Ordering::Release);
    }
}
impl Drop for TextureLease {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
        let _ = self.0.wake.try_send(());
    }
}

pub(super) struct TexturePool {
    resources: Vec<Arc<TextureResource>>,
    retired_rx: async_channel::Receiver<RetiredTexture>,
    context: gdk::GLContext,
    gl: GlFns,
    dimensions: [Cell<(i32, i32)>; SLOT_COUNT],
    signals: Rc<Signals>,
    wake: async_channel::Sender<()>,
}
impl TexturePool {
    /// The caller has made `context` current. No GPU allocation per steady-state
    /// frame; each slot is resized only after its previous GDK lease is released.
    pub(super) fn new(context: &gdk::GLContext, gl: GlFns) -> Self {
        let mut ids = [0; SLOT_COUNT];
        unsafe { (gl.gen_textures)(SLOT_COUNT as i32, ids.as_mut_ptr()) };
        let (retired, retired_rx) = async_channel::bounded::<RetiredTexture>(SLOT_COUNT);
        let (wake, wake_rx) = async_channel::bounded::<()>(1);
        let signals = Rc::new(Signals::default());
        let weak = Rc::downgrade(&signals);
        glib::spawn_future_local(async move {
            while wake_rx.recv().await.is_ok() {
                let Some(signals) = weak.upgrade() else { break };
                if signals.pending.replace(false) || signals.force.get() {
                    let callback = signals.redraw.borrow().clone();
                    if let Some(callback) = callback {
                        callback();
                    }
                }
            }
        });
        // This future has no Sender and no pointer back to the pool. After the
        // renderer AND the final GDK texture disappear the channel closes.
        let owned_context = context.clone();
        let retired_for_future = retired_rx.clone();
        glib::spawn_future_local(async move {
            while let Ok((id, fence)) = retired_for_future.recv().await {
                let _current = CurrentContext::enter(&owned_context);
                unsafe {
                    if fence != 0
                        && let Some(delete) = gl.delete_sync
                    {
                        delete(fence as *mut std::ffi::c_void);
                    }
                    (gl.delete_textures)(1, &raw const id);
                }
            }
        });
        Self {
            resources: (0..SLOT_COUNT)
                .map(|i| {
                    Arc::new(TextureResource {
                        id: ids[i],
                        busy: AtomicBool::new(false),
                        fence: AtomicUsize::new(0),
                        retired: retired.clone(),
                        wake: wake.clone(),
                    })
                })
                .collect(),
            retired_rx,
            context: context.clone(),
            gl,
            dimensions: std::array::from_fn(|_| Cell::new((0, 0))),
            signals,
            wake,
        }
    }

    pub(super) fn acquire(&self, gl: &GlFns) -> Option<(usize, TextureLease)> {
        for (i, resource) in self.resources.iter().enumerate() {
            if resource
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                // The previous GDK consumer is finished; its fence may now go.
                let fence = resource.fence.swap(0, Ordering::AcqRel);
                if fence != 0
                    && let Some(delete) = gl.delete_sync
                {
                    unsafe { delete(fence as *mut std::ffi::c_void) };
                }
                return Some((i, TextureLease(Arc::clone(resource))));
            }
        }
        self.signals.pending.set(true);
        self.signals.force.set(true); // update() may already have consumed FRAME
        None
    }
    pub(super) fn dimensions(&self, index: usize) -> (i32, i32) {
        self.dimensions[index].get()
    }
    pub(super) fn set_dimensions(&self, index: usize, width: i32, height: i32) {
        self.dimensions[index].set((width, height));
    }
    pub(super) fn take_force(&self) -> bool {
        self.signals.force.replace(false)
    }
    pub(super) fn force(&self) {
        self.signals.force.set(true);
    }
    pub(super) fn set_redraw(&self, callback: Rc<dyn Fn()>) {
        self.signals.redraw.replace(Some(callback));
    }
    pub(super) fn watch_resize(&self, probe: &gtk::DrawingArea) {
        let signals = Rc::downgrade(&self.signals);
        let wake = self.wake.clone();
        probe.connect_resize(move |_, _, _| {
            if let Some(signals) = signals.upgrade() {
                signals.force.set(true);
                let _ = wake.try_send(());
            }
        });
    }
}

impl Drop for TexturePool {
    fn drop(&mut self) {
        self.signals.redraw.borrow_mut().take();
        // Retire every unleased resource synchronously while the context exists.
        // Truly leased textures remain alive; their receiver retains the context
        // for a later GTK-loop iteration. The host must keep that loop running
        // until external paintables are released; never free still-busy textures.
        self.resources.clear();
        let _current = CurrentContext::enter(&self.context);
        while let Ok((id, fence)) = self.retired_rx.try_recv() {
            unsafe {
                if fence != 0
                    && let Some(delete) = self.gl.delete_sync
                {
                    delete(fence as *mut std::ffi::c_void);
                }
                (self.gl.delete_textures)(1, &raw const id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_survives_renderer_until_gdk_releases_lease() {
        let (tx, rx) = async_channel::bounded(3);
        let (wake, wake_rx) = async_channel::bounded(1);
        let resource = Arc::new(TextureResource {
            id: 42,
            busy: AtomicBool::new(true),
            fence: AtomicUsize::new(7),
            retired: tx,
            wake,
        });
        let lease = TextureLease(resource.clone());
        drop(resource); // renderer drops first
        assert!(rx.try_recv().is_err());
        drop(lease); // GDK releases later
        assert!(wake_rx.try_recv().is_ok());
        assert_eq!(rx.try_recv().unwrap(), (42, 7));
    }

    #[test]
    fn release_token_is_send_without_gtk_objects() {
        fn assert_send<T: Send>() {}
        assert_send::<TextureLease>();
    }
}
