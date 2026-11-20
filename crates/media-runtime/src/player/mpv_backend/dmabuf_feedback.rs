//! The compositor's `linux-dmabuf` feedback for one surface, read on a private
//! queue of GDK's Wayland connection. GDK keeps the feedback it reads to itself
//! and attaches offloaded buffers without checking them, so the offload renderer
//! asks for its window's own to allocate frames in a layout the compositor takes.
//!
//! The two protocol interfaces are declared here against libwayland-client, which
//! GTK already loaded; only the requests and events this reader uses matter.

use super::gl_proc_address::{SystemLibrary, library_function, library_symbol};
use log::debug;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

const DRM_FORMAT_MOD_LINEAR: u64 = 0;
const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;
const TRANCHE_FLAGS_SCANOUT: u32 = 1;
/// 64 Ki entries of 16 bytes; compositors send a few hundred.
const MAX_FORMAT_TABLE_BYTES: usize = 1 << 20;
const FORMAT_TABLE_ENTRY_BYTES: usize = 16;
const MAX_TRANCHES: usize = 64;
/// `get_surface_feedback` exists from version 4.
const DMABUF_VERSION: u32 = 4;
const MAX_ROUNDTRIPS: usize = 4;
const WL_MARSHAL_FLAG_DESTROY: u32 = 1;

/// One preference tranche: its flags and its indices into the format table.
#[derive(Debug, Default, Clone)]
pub(crate) struct Tranche {
    flags: u32,
    indices: Vec<u16>,
}

#[derive(Debug, Default)]
pub(crate) struct SurfaceFeedback {
    table: Vec<(u32, u64)>,
    tranches: Vec<Tranche>,
}

/// Modifiers to allocate with, in the compositor's order of preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) modifiers: Vec<u64>,
    /// The tranche names a plane that can show the buffer as it is.
    pub(crate) scanout: bool,
}

/// `(fourcc, modifier)` pairs of the table the compositor shares, `None` for a
/// size that is not whole entries or over [`MAX_FORMAT_TABLE_BYTES`].
fn parse_format_table(bytes: &[u8]) -> Option<Vec<(u32, u64)>> {
    if bytes.len() > MAX_FORMAT_TABLE_BYTES {
        return None;
    }
    let (entries, rest) = bytes.as_chunks::<FORMAT_TABLE_ENTRY_BYTES>();
    if !rest.is_empty() {
        return None;
    }
    let mut table = Vec::with_capacity(entries.len());
    for entry in entries {
        let (format, entry) = entry.split_first_chunk::<4>()?;
        let (_padding, modifier) = entry.split_first_chunk::<4>()?;
        let modifier: [u8; 8] = modifier.try_into().ok()?;
        table.push((u32::from_ne_bytes(*format), u64::from_ne_bytes(modifier)));
    }
    Some(table)
}

/// The first tranche, in the compositor's order, that lists `fourcc` with a
/// modifier the device renders to (`renderable`, or linear). Indices past the
/// table and the implicit modifier are skipped.
pub(crate) fn choose_layout(
    feedback: &SurfaceFeedback,
    fourcc: u32,
    renderable: &[u64],
) -> Option<Layout> {
    for tranche in &feedback.tranches {
        let mut modifiers = Vec::new();
        for &index in &tranche.indices {
            let Some(&(format, modifier)) = feedback.table.get(usize::from(index)) else {
                continue;
            };
            let usable = modifier == DRM_FORMAT_MOD_LINEAR || renderable.contains(&modifier);
            if format == fourcc
                && modifier != DRM_FORMAT_MOD_INVALID
                && usable
                && !modifiers.contains(&modifier)
            {
                modifiers.push(modifier);
            }
        }
        if !modifiers.is_empty() {
            return Some(Layout {
                modifiers,
                scanout: tranche.flags & TRANCHE_FLAGS_SCANOUT != 0,
            });
        }
    }
    None
}

#[repr(C)]
struct WlMessage {
    name: *const c_char,
    signature: *const c_char,
    types: *const *const WlInterface,
}

#[repr(C)]
struct WlInterface {
    name: *const c_char,
    version: c_int,
    method_count: c_int,
    methods: *const WlMessage,
    event_count: c_int,
    events: *const WlMessage,
}

#[repr(C)]
struct WlArray {
    size: usize,
    alloc: usize,
    data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
union WlArgument {
    int: i32,
    uint: u32,
    string: *const c_char,
    object: *mut c_void,
    array: *mut WlArray,
}

/// A static protocol table: libwayland only reads it.
struct Table<T>(T);
// SAFETY: the tables are immutable C data that no thread writes.
unsafe impl<T> Sync for Table<T> {}

const fn message(
    name: &'static CStr,
    signature: &'static CStr,
    types: *const *const WlInterface,
) -> WlMessage {
    WlMessage {
        name: name.as_ptr(),
        signature: signature.as_ptr(),
        types,
    }
}

/// Types of messages without object arguments: libwayland reads none of them.
static NO_TYPES: Table<[*const WlInterface; 4]> = Table([std::ptr::null(); 4]);
static FEEDBACK_NEW_ID: Table<[*const WlInterface; 2]> =
    Table([&raw const FEEDBACK_INTERFACE.0, std::ptr::null()]);

static DMABUF_REQUESTS: Table<[WlMessage; 4]> = Table([
    message(c"destroy", c"", NO_TYPES.0.as_ptr()),
    message(c"create_params", c"n", NO_TYPES.0.as_ptr()),
    message(c"get_default_feedback", c"4n", FEEDBACK_NEW_ID.0.as_ptr()),
    message(c"get_surface_feedback", c"4no", FEEDBACK_NEW_ID.0.as_ptr()),
]);
static DMABUF_EVENTS: Table<[WlMessage; 2]> = Table([
    message(c"format", c"u", NO_TYPES.0.as_ptr()),
    message(c"modifier", c"3uuu", NO_TYPES.0.as_ptr()),
]);
static DMABUF_INTERFACE: Table<WlInterface> = Table(WlInterface {
    name: c"zwp_linux_dmabuf_v1".as_ptr(),
    version: DMABUF_VERSION as c_int,
    method_count: 4,
    methods: DMABUF_REQUESTS.0.as_ptr(),
    event_count: 2,
    events: DMABUF_EVENTS.0.as_ptr(),
});

static FEEDBACK_REQUESTS: Table<[WlMessage; 1]> =
    Table([message(c"destroy", c"", NO_TYPES.0.as_ptr())]);
static FEEDBACK_EVENTS: Table<[WlMessage; 7]> = Table([
    message(c"done", c"", NO_TYPES.0.as_ptr()),
    message(c"format_table", c"hu", NO_TYPES.0.as_ptr()),
    message(c"main_device", c"a", NO_TYPES.0.as_ptr()),
    message(c"tranche_done", c"", NO_TYPES.0.as_ptr()),
    message(c"tranche_target_device", c"a", NO_TYPES.0.as_ptr()),
    message(c"tranche_formats", c"a", NO_TYPES.0.as_ptr()),
    message(c"tranche_flags", c"u", NO_TYPES.0.as_ptr()),
]);
static FEEDBACK_INTERFACE: Table<WlInterface> = Table(WlInterface {
    name: c"zwp_linux_dmabuf_feedback_v1".as_ptr(),
    version: DMABUF_VERSION as c_int,
    method_count: 1,
    methods: FEEDBACK_REQUESTS.0.as_ptr(),
    event_count: 7,
    events: FEEDBACK_EVENTS.0.as_ptr(),
});

const WL_DISPLAY_GET_REGISTRY: u32 = 1;
const WL_REGISTRY_BIND: u32 = 0;
const WL_REGISTRY_GLOBAL: u32 = 0;
const DMABUF_GET_SURFACE_FEEDBACK: u32 = 3;
const DESTROY: u32 = 0;
const FEEDBACK_DONE: u32 = 0;
const FEEDBACK_FORMAT_TABLE: u32 = 1;
const FEEDBACK_TRANCHE_DONE: u32 = 3;
const FEEDBACK_TRANCHE_FORMATS: u32 = 5;
const FEEDBACK_TRANCHE_FLAGS: u32 = 6;

type Dispatcher = unsafe extern "C" fn(
    *const c_void,
    *mut c_void,
    u32,
    *const WlMessage,
    *mut WlArgument,
) -> c_int;

struct WaylandFns {
    create_queue: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    destroy_queue: unsafe extern "C" fn(*mut c_void),
    create_wrapper: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    destroy_wrapper: unsafe extern "C" fn(*mut c_void),
    set_queue: unsafe extern "C" fn(*mut c_void, *mut c_void),
    marshal: unsafe extern "C" fn(
        *mut c_void,
        u32,
        *const WlInterface,
        u32,
        u32,
        *mut WlArgument,
    ) -> *mut c_void,
    add_dispatcher:
        unsafe extern "C" fn(*mut c_void, Dispatcher, *const c_void, *mut c_void) -> c_int,
    get_version: unsafe extern "C" fn(*mut c_void) -> u32,
    destroy_proxy: unsafe extern "C" fn(*mut c_void),
    roundtrip_queue: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    registry_interface: *const WlInterface,
}

impl WaylandFns {
    fn load() -> Option<Self> {
        let library = SystemLibrary::WaylandClient;
        let registry_interface = library_symbol(library, c"wl_registry_interface");
        if registry_interface.is_null() {
            return None;
        }
        // SAFETY: each name is paired with its libwayland-client signature.
        unsafe {
            Some(Self {
                create_queue: library_function(library, c"wl_display_create_queue")?,
                destroy_queue: library_function(library, c"wl_event_queue_destroy")?,
                create_wrapper: library_function(library, c"wl_proxy_create_wrapper")?,
                destroy_wrapper: library_function(library, c"wl_proxy_wrapper_destroy")?,
                set_queue: library_function(library, c"wl_proxy_set_queue")?,
                marshal: library_function(library, c"wl_proxy_marshal_array_flags")?,
                add_dispatcher: library_function(library, c"wl_proxy_add_dispatcher")?,
                get_version: library_function(library, c"wl_proxy_get_version")?,
                destroy_proxy: library_function(library, c"wl_proxy_destroy")?,
                roundtrip_queue: library_function(library, c"wl_display_roundtrip_queue")?,
                registry_interface: registry_interface.cast(),
            })
        }
    }
}

/// What the dispatchers collect during the roundtrips.
#[derive(Default)]
struct Collected {
    dmabuf_global: Option<(u32, u32)>,
    table: Option<Vec<(u32, u64)>>,
    pending: Tranche,
    tranches: Vec<Tranche>,
    done: bool,
}

/// The bytes of a `wl_array` argument.
///
/// # Safety
/// `array` is null or a live `wl_array` for the duration of the borrow.
unsafe fn array_bytes<'a>(array: *const WlArray) -> &'a [u8] {
    // SAFETY: forwarded from the caller; libwayland gives `size` bytes at `data`.
    unsafe {
        match array.as_ref() {
            Some(array) if !array.data.is_null() && array.size > 0 => {
                std::slice::from_raw_parts(array.data.cast::<u8>(), array.size)
            }
            _ => &[],
        }
    }
}

/// Maps the shared table read-only and copies it out; the fd is closed on return.
fn read_format_table(fd: OwnedFd, size: u32) -> Option<Vec<(u32, u64)>> {
    let size = usize::try_from(size).ok()?;
    if size == 0 || size > MAX_FORMAT_TABLE_BYTES {
        return None;
    }
    // SAFETY: a private read-only mapping of the fd libwayland handed over;
    // it is unmapped below before the fd closes.
    unsafe {
        let mapping = libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd.as_raw_fd(),
            0,
        );
        if mapping == libc::MAP_FAILED {
            return None;
        }
        let table = parse_format_table(std::slice::from_raw_parts(mapping.cast::<u8>(), size));
        libc::munmap(mapping, size);
        table
    }
}

unsafe extern "C" fn registry_event(
    data: *const c_void,
    _proxy: *mut c_void,
    opcode: u32,
    _message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: `data` is the `Collected` that `read_surface_feedback` owns for
    // the roundtrips; the arguments follow the event's signature "usu".
    unsafe {
        let collected = &mut *data.cast_mut().cast::<Collected>();
        if opcode == WL_REGISTRY_GLOBAL {
            let interface = (*args.add(1)).string;
            if !interface.is_null() && CStr::from_ptr(interface) == c"zwp_linux_dmabuf_v1" {
                collected.dmabuf_global = Some(((*args).uint, (*args.add(2)).uint));
            }
        }
    }
    0
}

unsafe extern "C" fn feedback_event(
    data: *const c_void,
    _proxy: *mut c_void,
    opcode: u32,
    _message: *const WlMessage,
    args: *mut WlArgument,
) -> c_int {
    // SAFETY: as in `registry_event`; each opcode reads its own signature.
    unsafe {
        let collected = &mut *data.cast_mut().cast::<Collected>();
        match opcode {
            FEEDBACK_DONE => collected.done = true,
            FEEDBACK_FORMAT_TABLE => {
                // libwayland hands the received descriptor over to the handler.
                let fd = OwnedFd::from_raw_fd((*args).int);
                collected.table = read_format_table(fd, (*args.add(1)).uint);
            }
            FEEDBACK_TRANCHE_FORMATS => {
                let (indices, _) = array_bytes((*args).array).as_chunks::<2>();
                collected.pending.indices = indices
                    .iter()
                    .map(|index| u16::from_ne_bytes(*index))
                    .collect();
            }
            FEEDBACK_TRANCHE_FLAGS => collected.pending.flags = (*args).uint,
            FEEDBACK_TRANCHE_DONE => {
                let tranche = std::mem::take(&mut collected.pending);
                if collected.tranches.len() < MAX_TRANCHES {
                    collected.tranches.push(tranche);
                }
            }
            _ => {}
        }
    }
    0
}

/// The proxies and queue of one read, destroyed in reverse order on every path.
struct Session<'a> {
    fns: &'a WaylandFns,
    queue: *mut c_void,
    wrapper: *mut c_void,
    registry: *mut c_void,
    dmabuf: *mut c_void,
    feedback: *mut c_void,
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        let mut no_args: [WlArgument; 0] = [];
        // SAFETY: each pointer is null or a proxy this session created and
        // still owns; destroy requests carry no arguments.
        unsafe {
            for proxy in [self.feedback, self.dmabuf] {
                if !proxy.is_null() {
                    let version = (self.fns.get_version)(proxy);
                    (self.fns.marshal)(
                        proxy,
                        DESTROY,
                        std::ptr::null(),
                        version,
                        WL_MARSHAL_FLAG_DESTROY,
                        no_args.as_mut_ptr(),
                    );
                }
            }
            if !self.registry.is_null() {
                (self.fns.destroy_proxy)(self.registry);
            }
            if !self.wrapper.is_null() {
                (self.fns.destroy_wrapper)(self.wrapper);
            }
            if !self.queue.is_null() {
                (self.fns.destroy_queue)(self.queue);
            }
        }
    }
}

impl<'a> Session<'a> {
    /// A private queue on `wl_display` and the registry on it, its globals read.
    ///
    /// # Safety
    /// `wl_display` is live; `collected` outlives the session.
    unsafe fn open(
        fns: &'a WaylandFns,
        wl_display: *mut c_void,
        collected: *mut Collected,
    ) -> Option<Self> {
        // SAFETY: forwarded from the caller; every proxy made here goes on the
        // private queue and is destroyed by `Drop`.
        unsafe {
            let mut session = Self {
                fns,
                queue: (fns.create_queue)(wl_display),
                wrapper: std::ptr::null_mut(),
                registry: std::ptr::null_mut(),
                dmabuf: std::ptr::null_mut(),
                feedback: std::ptr::null_mut(),
            };
            if session.queue.is_null() {
                return None;
            }
            session.wrapper = (fns.create_wrapper)(wl_display);
            if session.wrapper.is_null() {
                return None;
            }
            (fns.set_queue)(session.wrapper, session.queue);
            let mut new_registry = [WlArgument {
                object: std::ptr::null_mut(),
            }];
            session.registry = (fns.marshal)(
                session.wrapper,
                WL_DISPLAY_GET_REGISTRY,
                fns.registry_interface,
                (fns.get_version)(session.wrapper),
                0,
                new_registry.as_mut_ptr(),
            );
            let listening = !session.registry.is_null()
                && (fns.add_dispatcher)(
                    session.registry,
                    registry_event,
                    collected.cast(),
                    std::ptr::null_mut(),
                ) == 0;
            (listening && (fns.roundtrip_queue)(wl_display, session.queue) >= 0).then_some(session)
        }
    }

    /// Bind `zwp_linux_dmabuf_v1` (named `name` in the registry) and ask for the
    /// feedback of `wl_surface`.
    ///
    /// # Safety
    /// As [`Session::open`]; `wl_surface` is a live surface of the display.
    unsafe fn request_feedback(
        &mut self,
        name: u32,
        wl_surface: *mut c_void,
        collected: *mut Collected,
    ) -> Option<()> {
        // SAFETY: forwarded from the caller; the bind arguments follow
        // wl_registry.bind's "usun" and the feedback request's "no".
        unsafe {
            let mut bind = [
                WlArgument { uint: name },
                WlArgument {
                    string: DMABUF_INTERFACE.0.name,
                },
                WlArgument {
                    uint: DMABUF_VERSION,
                },
                WlArgument {
                    object: std::ptr::null_mut(),
                },
            ];
            self.dmabuf = (self.fns.marshal)(
                self.registry,
                WL_REGISTRY_BIND,
                &raw const DMABUF_INTERFACE.0,
                DMABUF_VERSION,
                0,
                bind.as_mut_ptr(),
            );
            if self.dmabuf.is_null() {
                return None;
            }
            let mut get_feedback = [
                WlArgument {
                    object: std::ptr::null_mut(),
                },
                WlArgument { object: wl_surface },
            ];
            self.feedback = (self.fns.marshal)(
                self.dmabuf,
                DMABUF_GET_SURFACE_FEEDBACK,
                &raw const FEEDBACK_INTERFACE.0,
                DMABUF_VERSION,
                0,
                get_feedback.as_mut_ptr(),
            );
            let listening = !self.feedback.is_null()
                && (self.fns.add_dispatcher)(
                    self.feedback,
                    feedback_event,
                    collected.cast(),
                    std::ptr::null_mut(),
                ) == 0;
            listening.then_some(())
        }
    }
}

/// The feedback the compositor gives `wl_surface`, after one `done`. `None`
/// when libwayland-client, the global at version 4 or the answer is missing.
///
/// # Safety
/// `wl_display` is GDK's live `wl_display` and `wl_surface` a live surface of
/// it, both used on this thread only.
pub(crate) unsafe fn read_surface_feedback(
    wl_display: *mut c_void,
    wl_surface: *mut c_void,
) -> Option<SurfaceFeedback> {
    let fns = WaylandFns::load()?;
    let mut collected = Collected::default();
    let collected_ptr = &raw mut collected;
    // SAFETY: the display and surface are live (caller), and `session` is
    // dropped, destroying the proxies whose dispatchers write through
    // `collected_ptr`, before `collected` is read whole.
    unsafe {
        let mut session = Session::open(&fns, wl_display, collected_ptr)?;
        // allow-noisy-log: debug-level reasons the window's layout falls back.
        let Some((name, advertised)) = (*collected_ptr).dmabuf_global else {
            debug!("dma-buf feedback: no zwp_linux_dmabuf_v1 global");
            return None;
        };
        if advertised < DMABUF_VERSION {
            debug!(
                "dma-buf feedback: zwp_linux_dmabuf_v1 version {advertised} has no surface feedback"
            );
            return None;
        }
        session.request_feedback(name, wl_surface, collected_ptr)?;
        for _ in 0..MAX_ROUNDTRIPS {
            if (*collected_ptr).done || (fns.roundtrip_queue)(wl_display, session.queue) < 0 {
                break;
            }
        }
        drop(session);
    }
    if !collected.done {
        debug!("dma-buf feedback: no done event");
        return None;
    }
    Some(SurfaceFeedback {
        table: collected.table?,
        tranches: collected.tranches,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        DRM_FORMAT_MOD_INVALID, DRM_FORMAT_MOD_LINEAR, Layout, MAX_FORMAT_TABLE_BYTES,
        SurfaceFeedback, Tranche, choose_layout, parse_format_table,
    };

    const XB24: u32 = 0x3432_4258;
    const AR24: u32 = 0x3432_5241;
    const Y_TILED: u64 = 0x0100_0000_0000_0002;
    const X_TILED: u64 = 0x0100_0000_0000_0001;

    fn entry(format: u32, modifier: u64) -> Vec<u8> {
        let mut bytes = format.to_ne_bytes().to_vec();
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&modifier.to_ne_bytes());
        bytes
    }

    #[test]
    fn format_table_parses_whole_entries() {
        let mut bytes = entry(XB24, DRM_FORMAT_MOD_LINEAR);
        bytes.extend(entry(AR24, Y_TILED));
        assert_eq!(
            parse_format_table(&bytes),
            Some(vec![(XB24, DRM_FORMAT_MOD_LINEAR), (AR24, Y_TILED)])
        );
        assert_eq!(parse_format_table(&[]), Some(Vec::new()));
        assert_eq!(parse_format_table(&bytes[..17]), None, "a partial entry");
        assert_eq!(
            parse_format_table(&vec![0; MAX_FORMAT_TABLE_BYTES]).map(|table| table.len()),
            Some(MAX_FORMAT_TABLE_BYTES / 16)
        );
        assert_eq!(
            parse_format_table(&vec![0; MAX_FORMAT_TABLE_BYTES + 16]),
            None,
            "over the cap"
        );
    }

    fn tranche(flags: u32, indices: &[u16]) -> Tranche {
        Tranche {
            flags,
            indices: indices.to_vec(),
        }
    }

    fn layout(modifiers: &[u64], scanout: bool) -> Layout {
        Layout {
            modifiers: modifiers.to_vec(),
            scanout,
        }
    }

    #[test]
    fn layout_follows_tranche_order() {
        let table = vec![
            (XB24, Y_TILED),
            (XB24, DRM_FORMAT_MOD_LINEAR),
            (AR24, DRM_FORMAT_MOD_LINEAR),
            (XB24, X_TILED),
            (XB24, DRM_FORMAT_MOD_INVALID),
        ];
        for (name, tranches, renderable, expected) in [
            (
                "2d: linear only",
                vec![tranche(0, &[1, 2])],
                vec![Y_TILED],
                Some(layout(&[DRM_FORMAT_MOD_LINEAR], false)),
            ),
            (
                "scanout tranche first",
                vec![tranche(1, &[3, 1]), tranche(0, &[0, 1])],
                vec![Y_TILED, X_TILED],
                Some(layout(&[X_TILED, DRM_FORMAT_MOD_LINEAR], true)),
            ),
            (
                "unrenderable modifiers move to the next tranche",
                vec![tranche(1, &[0]), tranche(0, &[1])],
                vec![],
                Some(layout(&[DRM_FORMAT_MOD_LINEAR], false)),
            ),
            (
                "index past the table, implicit modifier and duplicates skipped",
                vec![tranche(0, &[40, 4, 0, 0])],
                vec![Y_TILED],
                Some(layout(&[Y_TILED], false)),
            ),
            ("no XB24 at all", vec![tranche(0, &[2])], vec![], None),
            ("no tranches", vec![], vec![Y_TILED], None),
        ] {
            let feedback = SurfaceFeedback {
                table: table.clone(),
                tranches,
            };
            assert_eq!(
                choose_layout(&feedback, XB24, &renderable),
                expected,
                "{name}"
            );
        }
    }
}
