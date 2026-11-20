//! Offload slots allocated through GBM in a layout the compositor takes for the
//! window, from its `linux-dmabuf` feedback, instead of the layout the driver
//! picks for a GL texture (Y-tiled on Sandy Bridge, which big-compositor's 2d
//! refuses). Any step that fails leaves the slot to `glTexImage2D` and export.

use super::super::dmabuf_feedback::{self, FeedbackSession, Layout};
use super::super::egl_dmabuf::{self, DmaBufExport, EglDmaBufFns};
use super::super::gl_proc_address::{SystemLibrary, library_function};
use super::gl_fns::GlFns;
use super::texture_pool::SLOT_COUNT;
use gtk::prelude::*;
use log::{debug, info};
use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::{CStr, c_int, c_void};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::ptr::NonNull;

const DRM_FORMAT_XBGR8888: u32 = u32::from_le_bytes(*b"XB24");
const GBM_BO_USE_SCANOUT: u32 = 1 << 0;
const GBM_BO_USE_RENDERING: u32 = 1 << 2;

struct GbmFns {
    create_device: unsafe extern "C" fn(c_int) -> *mut c_void,
    destroy_device: unsafe extern "C" fn(*mut c_void),
    create_bo:
        unsafe extern "C" fn(*mut c_void, u32, u32, u32, *const u64, u32, u32) -> *mut c_void,
    destroy_bo: unsafe extern "C" fn(*mut c_void),
    plane_count: unsafe extern "C" fn(*mut c_void) -> c_int,
    modifier_plane_count: unsafe extern "C" fn(*mut c_void, u32, u64) -> c_int,
    fd: unsafe extern "C" fn(*mut c_void) -> c_int,
    stride: unsafe extern "C" fn(*mut c_void) -> u32,
    offset: unsafe extern "C" fn(*mut c_void, c_int) -> u32,
    modifier: unsafe extern "C" fn(*mut c_void) -> u64,
}

impl GbmFns {
    fn load() -> Option<Self> {
        let library = SystemLibrary::Gbm;
        // SAFETY: each name is paired with its libgbm signature.
        unsafe {
            Some(Self {
                create_device: library_function(library, c"gbm_create_device")?,
                destroy_device: library_function(library, c"gbm_device_destroy")?,
                create_bo: library_function(library, c"gbm_bo_create_with_modifiers2")?,
                destroy_bo: library_function(library, c"gbm_bo_destroy")?,
                plane_count: library_function(library, c"gbm_bo_get_plane_count")?,
                modifier_plane_count: library_function(
                    library,
                    c"gbm_device_get_format_modifier_plane_count",
                )?,
                fd: library_function(library, c"gbm_bo_get_fd")?,
                stride: library_function(library, c"gbm_bo_get_stride")?,
                offset: library_function(library, c"gbm_bo_get_offset")?,
                modifier: library_function(library, c"gbm_bo_get_modifier")?,
            })
        }
    }
}

/// A GBM device on the render node GL draws on. `Drop` destroys the device
/// before the fields drop, so the node closes after it.
struct Allocator {
    fns: GbmFns,
    device: NonNull<c_void>,
    _node: OwnedFd,
}

impl Allocator {
    fn open(path: &CStr) -> Option<Self> {
        let fns = GbmFns::load()?;
        let node: OwnedFd = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(std::ffi::OsStr::from_bytes(path.to_bytes()))
            .ok()?
            .into();
        // SAFETY: an open render node; GBM borrows it for the device's life.
        let device = NonNull::new(unsafe { (fns.create_device)(node.as_raw_fd()) })?;
        Some(Self {
            fns,
            device,
            _node: node,
        })
    }

    /// One single-plane XB24 buffer in one of `layout`'s modifiers, or why
    /// none could be made.
    fn allocate(&self, width: u32, height: u32, layout: &Layout) -> Result<DmaBufExport, String> {
        let modifiers = single_plane_modifiers(&layout.modifiers, |modifier| {
            // SAFETY: a live device; the query reads only its arguments.
            unsafe {
                (self.fns.modifier_plane_count)(self.device.as_ptr(), DRM_FORMAT_XBGR8888, modifier)
            }
        });
        if modifiers.is_empty() {
            return Err(format!(
                "none of the compositor's modifiers {:X?} lays XB24 out in one plane",
                layout.modifiers
            ));
        }
        let count = u32::try_from(modifiers.len())
            .map_err(|_| format!("{} modifiers are more than GBM takes", modifiers.len()))?;
        let usage = if layout.scanout {
            GBM_BO_USE_RENDERING | GBM_BO_USE_SCANOUT
        } else {
            GBM_BO_USE_RENDERING
        };
        // SAFETY: a live device; the bo is destroyed on every path once its
        // dma-buf fd, which keeps the memory alive, is taken.
        unsafe {
            let bo = (self.fns.create_bo)(
                self.device.as_ptr(),
                width,
                height,
                DRM_FORMAT_XBGR8888,
                modifiers.as_ptr(),
                count,
                usage,
            );
            if bo.is_null() {
                return Err(format!(
                    "GBM refused the single-plane modifiers {modifiers:X?}"
                ));
            }
            let planes = (self.fns.plane_count)(bo);
            let modifier = (self.fns.modifier)(bo);
            let export = (planes == 1).then(|| DmaBufExport {
                fd: (self.fns.fd)(bo),
                fourcc: DRM_FORMAT_XBGR8888,
                modifier,
                stride: (self.fns.stride)(bo),
                offset: (self.fns.offset)(bo, 0),
            });
            (self.fns.destroy_bo)(bo);
            match export {
                Some(export) if export.fd >= 0 => Ok(export),
                Some(_) => Err("GBM could not export its buffer as a dma-buf".to_owned()),
                None => Err(format!(
                    "GBM made modifier 0x{modifier:016X} in {planes} planes"
                )),
            }
        }
    }
}

/// `modifiers` that lay XB24 out in one plane, in the compositor's order. Given
/// a compression modifier, GBM picks it and puts the control surface in a
/// second plane, which a slot's single dma-buf cannot carry.
fn single_plane_modifiers(modifiers: &[u64], plane_count: impl Fn(u64) -> c_int) -> Vec<u64> {
    modifiers
        .iter()
        .copied()
        .filter(|&modifier| plane_count(modifier) == 1)
        .collect()
}

impl Drop for Allocator {
    fn drop(&mut self) {
        // SAFETY: the device was created by this allocator and no bo outlives
        // `allocate`.
        unsafe { (self.fns.destroy_device)(self.device.as_ptr()) };
    }
}

/// The slots' GBM buffers and the layout they were made in.
#[derive(Default)]
pub(super) struct SlotBuffers {
    allocator: OnceCell<Option<Allocator>>,
    /// Opened on the first read; `None` stays for the renderer's life.
    feedback: OnceCell<Option<FeedbackSession>>,
    buffers: RefCell<[Option<DmaBufExport>; SLOT_COUNT]>,
    /// The size the layout was read for, and the layout (`None`: driver's).
    layout: RefCell<Option<((i32, i32), Option<Layout>)>>,
    /// Logged when it changes, so a fallback is reported once.
    described: RefCell<String>,
    disabled: Cell<bool>,
}

impl SlotBuffers {
    /// Bind slot `index`'s `texture` to a new buffer in the compositor's layout
    /// for `picture`'s window. `false` leaves the texture to the caller.
    pub(super) fn bind(
        &self,
        index: usize,
        texture: u32,
        width: i32,
        height: i32,
        picture: &gtk::Picture,
        egl: Option<&EglDmaBufFns>,
        gl: &GlFns,
    ) -> bool {
        self.release(index);
        if self.disabled.get() {
            return false;
        }
        let Some(egl) = egl else { return false };
        let Some(import) = egl.import.as_ref() else {
            return self.describe_fallback("EGL cannot import dma-bufs or name its device");
        };
        let Some(layout) = self.layout_for(picture, egl, (width, height)) else {
            return false;
        };
        let Some(allocator) = self.allocator.get_or_init(|| {
            egl_dmabuf::render_node(egl, import).and_then(|path| Allocator::open(&path))
        }) else {
            return self.describe_fallback("no GBM device on GL's render node");
        };
        let (Ok(buffer_width), Ok(buffer_height)) = (u32::try_from(width), u32::try_from(height))
        else {
            return false;
        };
        let buffer = match allocator.allocate(buffer_width, buffer_height, &layout) {
            Ok(buffer) => buffer,
            Err(reason) => return self.describe_fallback(&reason),
        };
        if !egl_dmabuf::bind_dmabuf_texture(
            egl,
            import,
            &buffer,
            width,
            height,
            texture,
            gl.bind_texture,
            gl.get_error,
        ) {
            // A driver that refuses one GBM buffer refuses the next.
            self.disabled.set(true);
            return self.describe_fallback("EGL refused the GBM buffer");
        }
        self.describe(&format!(
            "XB24 modifier 0x{:016X}{} from the window's dma-buf feedback",
            buffer.modifier,
            if layout.scanout { ", scanout" } else { "" }
        ));
        self.buffers.borrow_mut()[index] = Some(buffer);
        true
    }

    /// A new descriptor of slot `index`'s buffer for GDK, `None` for a slot
    /// that renders into a plain GL texture.
    pub(super) fn export(&self, index: usize) -> Option<DmaBufExport> {
        let buffers = self.buffers.borrow();
        let buffer = buffers.get(index)?.as_ref()?;
        // SAFETY: duplicating a descriptor this slot owns.
        let fd = unsafe { libc::fcntl(buffer.fd, libc::F_DUPFD_CLOEXEC, 0) };
        (fd >= 0).then(|| DmaBufExport {
            fd,
            fourcc: buffer.fourcc,
            modifier: buffer.modifier,
            stride: buffer.stride,
            offset: buffer.offset,
        })
    }

    /// Stop allocating: the GL texture of a GBM buffer could not be rendered to.
    pub(super) fn disable(&self, index: usize) {
        self.release(index);
        self.disabled.set(true);
        self.describe("driver layout: the GBM buffer is no render target");
    }

    fn release(&self, index: usize) {
        if let Some(slot) = self.buffers.borrow_mut().get_mut(index) {
            *slot = None;
        }
    }

    /// The layout for this size, read from the window's feedback once per size.
    fn layout_for(
        &self,
        picture: &gtk::Picture,
        egl: &EglDmaBufFns,
        size: (i32, i32),
    ) -> Option<Layout> {
        if let Some((read_for, layout)) = self.layout.borrow().as_ref()
            && *read_for == size
        {
            return layout.clone();
        }
        let layout = self.read_layout(picture, egl);
        *self.layout.borrow_mut() = Some((size, layout.clone()));
        layout
    }

    fn read_layout(&self, picture: &gtk::Picture, egl: &EglDmaBufFns) -> Option<Layout> {
        let import = egl.import.as_ref()?;
        let surface = picture.native().and_then(|native| native.surface());
        let wl_display = super::super::video_surface::wayland_display_ptr(&picture.display());
        let wl_surface = surface
            .as_ref()
            .and_then(super::super::video_surface::wayland_surface_ptr);
        let (Some(wl_display), Some(wl_surface)) = (wl_display, wl_surface) else {
            self.describe("driver layout: the window has no Wayland surface");
            return None;
        };
        // SAFETY: GDK's display, which outlives the renderer, and the window's
        // surface, alive while the picture is realized, both on the main thread.
        let session = self
            .feedback
            .get_or_init(|| unsafe { FeedbackSession::open(wl_display) });
        let Some(session) = session else {
            self.describe("driver layout: the compositor offers no dma-buf feedback");
            return None;
        };
        // SAFETY: as above.
        let Some(feedback) = (unsafe { session.read(wl_surface) }) else {
            self.describe("driver layout: the compositor gave no dma-buf feedback for the window");
            return None;
        };
        let renderable = egl_dmabuf::renderable_modifiers(egl, import, DRM_FORMAT_XBGR8888);
        let layout = dmabuf_feedback::choose_layout(&feedback, DRM_FORMAT_XBGR8888, &renderable);
        debug!(
            "Offload slots: window feedback {feedback:?}, GL renders to {renderable:X?}: {layout:X?}"
        );
        if layout.is_none() {
            self.describe("driver layout: the window's feedback lists no XB24 GL renders to");
        }
        layout
    }

    /// Report why the slot stays a plain GL texture; always `false`.
    fn describe_fallback(&self, reason: &str) -> bool {
        self.describe(&format!("driver layout: {reason}"));
        false
    }

    fn describe(&self, description: &str) {
        let mut described = self.described.borrow_mut();
        if *described != description {
            // Once per change of the slots' layout.
            info!("Offload slots: {description}");
            description.clone_into(&mut described);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::single_plane_modifiers;
    use std::ffi::c_int;

    const LINEAR: u64 = 0;
    const X_TILED: u64 = 0x0100_0000_0000_0001;
    const Y_TILED: u64 = 0x0100_0000_0000_0002;
    const Y_TILED_CCS: u64 = 0x0100_0000_0000_0004;
    const TILE_4: u64 = 0x0100_0000_0000_0009;
    const MTL_RC_CCS: u64 = 0x0100_0000_0000_000D;
    const MTL_RC_CCS_CC: u64 = 0x0100_0000_0000_000F;
    const INVALID: u64 = 0x00FF_FFFF_FFFF_FFFF;

    /// XB24's planes per modifier as `drm_fourcc.h` defines them (CCS at plane
    /// 1, clear color at plane 2); -1 where GBM knows no count.
    fn planes(modifier: u64) -> c_int {
        match modifier {
            LINEAR | X_TILED | Y_TILED | TILE_4 => 1,
            Y_TILED_CCS | MTL_RC_CCS => 2,
            MTL_RC_CCS_CC => 3,
            _ => -1,
        }
    }

    /// The first two rows are the modifiers Kaby Lake and Arrow Lake windows
    /// offered for XB24 that GL also renders to.
    #[test]
    fn multi_plane_modifiers_dropped() {
        let cases: [(&[u64], &[u64]); 5] = [
            (
                &[LINEAR, X_TILED, Y_TILED, Y_TILED_CCS],
                &[LINEAR, X_TILED, Y_TILED],
            ),
            (
                &[LINEAR, X_TILED, TILE_4, MTL_RC_CCS, MTL_RC_CCS_CC],
                &[LINEAR, X_TILED, TILE_4],
            ),
            (&[MTL_RC_CCS_CC, Y_TILED, LINEAR], &[Y_TILED, LINEAR]),
            (&[Y_TILED_CCS, MTL_RC_CCS_CC, INVALID], &[]),
            (&[], &[]),
        ];
        for (offered, kept) in cases {
            assert_eq!(
                single_plane_modifiers(offered, planes),
                kept,
                "offered {offered:X?}"
            );
        }
    }
}
