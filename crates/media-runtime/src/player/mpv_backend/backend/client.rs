//! Minimal libmpv client over `libmpv2-sys`.
//!
//! The runtime uses the system `libmpv` C API directly here so consumers do
//! not pull the higher-level Rust `libmpv2` wrapper into every binary.

use std::ffi::{CStr, CString, NulError};
use std::fmt;
use std::os::raw::{c_char, c_int, c_ulong, c_void};
use std::ptr::NonNull;
use std::str::Utf8Error;

type Result<T> = std::result::Result<T, MpvError>;

const MPV_CLIENT_API_MAJOR: c_ulong = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpvError {
    InvalidUtf8,
    NulByte,
    NullHandle,
    DestroyedHandle,
    VersionMismatch { linked: c_ulong, loaded: c_ulong },
    Raw(c_int),
}

impl fmt::Display for MpvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => f.write_str("mpv returned invalid UTF-8"),
            Self::NulByte => f.write_str("mpv string contains interior nul byte"),
            Self::NullHandle => f.write_str("mpv returned a null handle"),
            Self::DestroyedHandle => f.write_str("mpv handle was already destroyed"),
            Self::VersionMismatch { linked, loaded } => {
                write!(
                    f,
                    "mpv client API major mismatch: linked={linked}, loaded={loaded}"
                )
            }
            Self::Raw(code) => write!(f, "mpv error {code}: {}", mpv_error_message(*code)),
        }
    }
}

impl std::error::Error for MpvError {}

impl From<NulError> for MpvError {
    fn from(_: NulError) -> Self {
        Self::NulByte
    }
}

impl From<Utf8Error> for MpvError {
    fn from(_: Utf8Error) -> Self {
        Self::InvalidUtf8
    }
}

pub struct MpvClient {
    mpv_handle: Option<NonNull<mpv_sys::mpv_handle>>,
}

// SAFETY: libmpv client handles are documented for cross-thread client API
// use; mutable access is still serialized by `MpvBackend`'s Mutex.
unsafe impl Send for MpvClient {}
unsafe impl Sync for MpvClient {}

impl MpvClient {
    pub fn with_initializer<F>(setup: F) -> Result<Self>
    where
        F: FnOnce(&MpvInitializer) -> Result<()>,
    {
        // SAFETY: version query is process-global and has no preconditions.
        let loaded = unsafe { mpv_sys::mpv_client_api_version() };
        let loaded_major = loaded >> 16;
        if loaded_major != MPV_CLIENT_API_MAJOR {
            return Err(MpvError::VersionMismatch {
                linked: MPV_CLIENT_API_MAJOR,
                loaded: loaded_major,
            });
        }

        // SAFETY: `mpv_create` returns either a valid handle or null.
        let mpv_handle =
            NonNull::new(unsafe { mpv_sys::mpv_create() }).ok_or(MpvError::NullHandle)?;
        let init = MpvInitializer { mpv_handle };
        if let Err(error) = setup(&init) {
            // SAFETY: `mpv_handle` was returned by `mpv_create` and is not initialized
            // for any other owner yet.
            unsafe {
                mpv_sys::mpv_terminate_destroy(mpv_handle.as_ptr());
            }
            return Err(error);
        }

        // SAFETY: `mpv_handle` is a valid mpv handle and initialization options were
        // applied only through mpv's client API.
        let result = unsafe { mpv_sys::mpv_initialize(mpv_handle.as_ptr()) };
        if result < 0 {
            // SAFETY: `mpv_handle` is still owned by this initializer path.
            unsafe {
                mpv_sys::mpv_terminate_destroy(mpv_handle.as_ptr());
            }
            return Err(MpvError::Raw(result));
        }

        Ok(Self {
            mpv_handle: Some(mpv_handle),
        })
    }

    pub fn raw_handle(&self) -> *mut mpv_sys::mpv_handle {
        self.mpv_handle
            .map_or(std::ptr::null_mut(), NonNull::as_ptr)
    }

    pub fn terminate_destroy(&mut self) {
        if let Some(mpv_handle) = self.mpv_handle.take() {
            // SAFETY: this object owns the handle; taking it prevents Drop from
            // destroying it again.
            unsafe {
                mpv_sys::mpv_terminate_destroy(mpv_handle.as_ptr());
            }
        }
    }

    pub fn command(&self, name: &str, args: &[&str]) -> Result<()> {
        let mpv_handle = self.mpv_handle()?;
        let mut c_strings = Vec::with_capacity(args.len() + 1);
        c_strings.push(CString::new(name)?);
        for arg in args {
            c_strings.push(CString::new(*arg)?);
        }

        let mut raw_args: Vec<_> = c_strings.iter().map(|arg| arg.as_ptr()).collect();
        raw_args.push(std::ptr::null());

        // SAFETY: `raw_args` is null-terminated and all C strings live until
        // the call returns.
        check_mpv(unsafe { mpv_sys::mpv_command(mpv_handle.as_ptr(), raw_args.as_mut_ptr()) })
    }

    pub fn set_property<T>(&self, name: &str, value: T) -> Result<()>
    where
        T: MpvSetValue,
    {
        let mpv_handle = self.mpv_handle()?;
        let name = CString::new(name)?;
        value.with_mpv_value(|format, data| {
            // SAFETY: `name` and `data` live for the duration of the call; the
            // format matches the concrete storage provided by `MpvSetValue`.
            check_mpv(unsafe {
                mpv_sys::mpv_set_property(mpv_handle.as_ptr(), name.as_ptr(), format, data)
            })
        })
    }

    pub fn get_property<T>(&self, name: &str) -> Result<T>
    where
        T: MpvGetValue,
    {
        let mpv_handle = self.mpv_handle()?;
        let name = CString::new(name)?;
        T::get(mpv_handle, name.as_c_str())
    }

    pub fn observe_property(&self, name: &str) -> Result<()> {
        let handle = self.mpv_handle()?;
        let name = CString::new(name)?;
        // SAFETY: the handle is live and name lives through this call. NONE
        // requests invalidation events, including when a property disappears.
        check_mpv(unsafe {
            mpv_sys::mpv_observe_property(
                handle.as_ptr(),
                0,
                name.as_ptr(),
                mpv_sys::mpv_format_MPV_FORMAT_NONE,
            )
        })
    }

    pub fn wait_event(&mut self, timeout: f64) -> Option<Result<MpvEvent>> {
        let Some(mpv_handle) = self.mpv_handle else {
            return Some(Err(MpvError::DestroyedHandle));
        };
        // SAFETY: `mpv_handle` is a live mpv handle owned by this client.
        let raw_event = unsafe { mpv_sys::mpv_wait_event(mpv_handle.as_ptr(), timeout) };
        let event = NonNull::new(raw_event).ok_or(MpvError::NullHandle);
        match event {
            Ok(event) => map_event(
                // SAFETY: libmpv returns a pointer valid until the next wait call.
                unsafe { event.as_ref() },
            ),
            Err(error) => Some(Err(error)),
        }
    }

    fn mpv_handle(&self) -> Result<NonNull<mpv_sys::mpv_handle>> {
        self.mpv_handle.ok_or(MpvError::DestroyedHandle)
    }
}

impl Drop for MpvClient {
    fn drop(&mut self) {
        if let Some(mpv_handle) = self.mpv_handle.take() {
            // SAFETY: fallback cleanup for initialization paths that are not
            // owned by `MpvBackend::drop`.
            unsafe {
                mpv_sys::mpv_destroy(mpv_handle.as_ptr());
            }
        }
    }
}

pub struct MpvInitializer {
    mpv_handle: NonNull<mpv_sys::mpv_handle>,
}

impl MpvInitializer {
    pub fn set_option<T>(&self, name: &str, value: T) -> Result<()>
    where
        T: MpvSetValue,
    {
        let name = CString::new(name)?;
        value.with_mpv_value(|format, data| {
            // SAFETY: `name` and `data` live for the duration of the call; the
            // format matches the concrete storage provided by `MpvSetValue`.
            check_mpv(unsafe {
                mpv_sys::mpv_set_option(self.mpv_handle.as_ptr(), name.as_ptr(), format, data)
            })
        })
    }
}

pub trait MpvSetValue {
    fn with_mpv_value<R>(
        self,
        call: impl FnOnce(mpv_sys::mpv_format, *mut c_void) -> Result<R>,
    ) -> Result<R>;
}

impl MpvSetValue for &str {
    fn with_mpv_value<R>(
        self,
        call: impl FnOnce(mpv_sys::mpv_format, *mut c_void) -> Result<R>,
    ) -> Result<R> {
        let string = CString::new(self)?;
        let mut raw = string.as_ptr();
        call(
            mpv_sys::mpv_format_MPV_FORMAT_STRING,
            std::ptr::from_mut(&mut raw).cast::<c_void>(),
        )
    }
}

impl MpvSetValue for String {
    fn with_mpv_value<R>(
        self,
        call: impl FnOnce(mpv_sys::mpv_format, *mut c_void) -> Result<R>,
    ) -> Result<R> {
        self.as_str().with_mpv_value(call)
    }
}

impl MpvSetValue for bool {
    fn with_mpv_value<R>(
        self,
        call: impl FnOnce(mpv_sys::mpv_format, *mut c_void) -> Result<R>,
    ) -> Result<R> {
        let mut value: c_int = i32::from(self);
        call(
            mpv_sys::mpv_format_MPV_FORMAT_FLAG,
            std::ptr::from_mut(&mut value).cast::<c_void>(),
        )
    }
}

impl MpvSetValue for i64 {
    fn with_mpv_value<R>(
        self,
        call: impl FnOnce(mpv_sys::mpv_format, *mut c_void) -> Result<R>,
    ) -> Result<R> {
        let mut value = self;
        call(
            mpv_sys::mpv_format_MPV_FORMAT_INT64,
            std::ptr::from_mut(&mut value).cast::<c_void>(),
        )
    }
}

impl MpvSetValue for f64 {
    fn with_mpv_value<R>(
        self,
        call: impl FnOnce(mpv_sys::mpv_format, *mut c_void) -> Result<R>,
    ) -> Result<R> {
        let mut value = self;
        call(
            mpv_sys::mpv_format_MPV_FORMAT_DOUBLE,
            std::ptr::from_mut(&mut value).cast::<c_void>(),
        )
    }
}

pub trait MpvGetValue: Sized {
    fn get(mpv_handle: NonNull<mpv_sys::mpv_handle>, name: &CStr) -> Result<Self>;
}

impl MpvGetValue for String {
    fn get(mpv_handle: NonNull<mpv_sys::mpv_handle>, name: &CStr) -> Result<Self> {
        let mut raw = std::ptr::null_mut::<c_char>();
        // SAFETY: `raw` is an out pointer for MPV_FORMAT_STRING; mpv allocates
        // the string and ownership is released with `mpv_free`.
        check_mpv(unsafe {
            mpv_sys::mpv_get_property(
                mpv_handle.as_ptr(),
                name.as_ptr(),
                mpv_sys::mpv_format_MPV_FORMAT_STRING,
                std::ptr::from_mut(&mut raw).cast::<c_void>(),
            )
        })?;
        if raw.is_null() {
            return Ok(String::new());
        }

        // SAFETY: on success mpv returns a valid nul-terminated string.
        let value = unsafe { CStr::from_ptr(raw) }.to_str()?.to_owned();
        // SAFETY: `raw` was allocated by mpv for this property read.
        unsafe {
            mpv_sys::mpv_free(raw.cast::<c_void>());
        }
        Ok(value)
    }
}

impl MpvGetValue for bool {
    fn get(mpv_handle: NonNull<mpv_sys::mpv_handle>, name: &CStr) -> Result<Self> {
        let mut value = 0_i32;
        // SAFETY: MPV_FORMAT_FLAG uses C `int` storage.
        check_mpv(unsafe {
            mpv_sys::mpv_get_property(
                mpv_handle.as_ptr(),
                name.as_ptr(),
                mpv_sys::mpv_format_MPV_FORMAT_FLAG,
                std::ptr::from_mut(&mut value).cast::<c_void>(),
            )
        })?;
        Ok(value != 0)
    }
}

impl MpvGetValue for i64 {
    fn get(mpv_handle: NonNull<mpv_sys::mpv_handle>, name: &CStr) -> Result<Self> {
        let mut value = 0_i64;
        // SAFETY: MPV_FORMAT_INT64 uses i64 storage.
        check_mpv(unsafe {
            mpv_sys::mpv_get_property(
                mpv_handle.as_ptr(),
                name.as_ptr(),
                mpv_sys::mpv_format_MPV_FORMAT_INT64,
                std::ptr::from_mut(&mut value).cast::<c_void>(),
            )
        })?;
        Ok(value)
    }
}

impl MpvGetValue for f64 {
    fn get(mpv_handle: NonNull<mpv_sys::mpv_handle>, name: &CStr) -> Result<Self> {
        let mut value = 0.0_f64;
        // SAFETY: MPV_FORMAT_DOUBLE uses f64 storage.
        check_mpv(unsafe {
            mpv_sys::mpv_get_property(
                mpv_handle.as_ptr(),
                name.as_ptr(),
                mpv_sys::mpv_format_MPV_FORMAT_DOUBLE,
                std::ptr::from_mut(&mut value).cast::<c_void>(),
            )
        })?;
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MpvPropertyValue {
    Invalidated,
    Flag(bool),
    Double(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MpvEvent {
    AudioReconfig,
    FileLoaded,
    EndFile(u32),
    Shutdown,
    PropertyChange {
        name: String,
        change: MpvPropertyValue,
    },
    CommandReply {
        id: u64,
        error: c_int,
    },
    Seek,
    PlaybackRestart,
    Other,
}

fn check_mpv(code: c_int) -> Result<()> {
    if code < 0 {
        Err(MpvError::Raw(code))
    } else {
        Ok(())
    }
}

fn mpv_error_message(code: c_int) -> String {
    // SAFETY: mpv returns a static nul-terminated string for known errors.
    let raw = unsafe { mpv_sys::mpv_error_string(code) };
    if raw.is_null() {
        return "unknown error".to_string();
    }
    // SAFETY: checked non-null above; invalid UTF-8 is rendered lossily for logs.
    unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}

fn map_event(event: &mpv_sys::mpv_event) -> Option<Result<MpvEvent>> {
    if event.event_id == mpv_sys::mpv_event_id_MPV_EVENT_NONE {
        return None;
    }
    if event.event_id == mpv_sys::mpv_event_id_MPV_EVENT_COMMAND_REPLY {
        return Some(Ok(MpvEvent::CommandReply {
            id: event.reply_userdata,
            error: event.error,
        }));
    }
    if event.error < 0 {
        return Some(Err(MpvError::Raw(event.error)));
    }

    Some(match event.event_id {
        mpv_sys::mpv_event_id_MPV_EVENT_AUDIO_RECONFIG => Ok(MpvEvent::AudioReconfig),
        mpv_sys::mpv_event_id_MPV_EVENT_FILE_LOADED => Ok(MpvEvent::FileLoaded),
        mpv_sys::mpv_event_id_MPV_EVENT_SEEK => Ok(MpvEvent::Seek),
        mpv_sys::mpv_event_id_MPV_EVENT_PLAYBACK_RESTART => Ok(MpvEvent::PlaybackRestart),
        mpv_sys::mpv_event_id_MPV_EVENT_END_FILE => Ok(map_end_file_event(event.data)),
        mpv_sys::mpv_event_id_MPV_EVENT_SHUTDOWN => Ok(MpvEvent::Shutdown),
        mpv_sys::mpv_event_id_MPV_EVENT_PROPERTY_CHANGE => map_property_event(event.data),
        _ => Ok(MpvEvent::Other),
    })
}

fn map_end_file_event(event_payload: *mut c_void) -> MpvEvent {
    if event_payload.is_null() {
        return MpvEvent::EndFile(0);
    }
    // SAFETY: MPV_EVENT_END_FILE carries `mpv_event_end_file`.
    let event = unsafe { &*event_payload.cast::<mpv_sys::mpv_event_end_file>() };
    MpvEvent::EndFile(if event.error < 0 { 4 } else { event.reason })
}

fn map_property_event(event_payload: *mut c_void) -> Result<MpvEvent> {
    if event_payload.is_null() {
        return Ok(MpvEvent::Other);
    }
    // SAFETY: MPV_EVENT_PROPERTY_CHANGE carries `mpv_event_property`.
    let property = unsafe { &*event_payload.cast::<mpv_sys::mpv_event_property>() };
    if property.name.is_null() {
        return Ok(MpvEvent::Other);
    }

    // SAFETY: mpv provides a valid property name for property-change events.
    let name = unsafe { CStr::from_ptr(property.name) }
        .to_str()?
        .to_owned();
    if property.format == mpv_sys::mpv_format_MPV_FORMAT_NONE {
        return Ok(MpvEvent::PropertyChange {
            name,
            change: MpvPropertyValue::Invalidated,
        });
    }
    if property.data.is_null() {
        return Ok(MpvEvent::Other);
    }
    let change = match property.format {
        mpv_sys::mpv_format_MPV_FORMAT_FLAG => {
            // SAFETY: MPV_FORMAT_FLAG uses C `int` storage.
            MpvPropertyValue::Flag(unsafe { *property.data.cast::<c_int>() } != 0)
        }
        mpv_sys::mpv_format_MPV_FORMAT_DOUBLE => {
            // SAFETY: MPV_FORMAT_DOUBLE uses f64 storage.
            MpvPropertyValue::Double(unsafe { *property.data.cast::<f64>() })
        }
        _ => return Ok(MpvEvent::Other),
    };

    Ok(MpvEvent::PropertyChange { name, change })
}
