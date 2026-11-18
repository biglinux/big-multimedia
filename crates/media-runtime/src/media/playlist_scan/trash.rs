//! Moving files to the trash and back through GIO. Every call blocks on the
//! file system or GVfs, so run it off the main loop.

use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use gio::prelude::*;
use glib::translate::ToGlibPtr;

use crate::error::{Error, Result};

const ORIGINAL_PATH: &CStr = c"trash::orig-path";

/// Move `path` to the trash.
pub fn trash_file(path: &Path) -> Result<()> {
    #[expect(
        clippy::disallowed_methods,
        reason = "this module owns trashing; see restore_from_trash"
    )]
    let trashed = gio::File::for_path(path).trash(gio::Cancellable::NONE);
    trashed.map_err(|error| Error::internal(error.to_string()))
}

/// Put back the item most recently trashed from `original_path`, refusing to
/// replace a file that is there now.
pub fn restore_from_trash(original_path: &Path) -> Result<()> {
    let gio_error = |error: glib::Error| Error::internal(error.to_string());
    let children = gio::File::for_uri("trash:///")
        .enumerate_children(
            "standard::name,trash::orig-path,trash::deletion-date",
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .map_err(gio_error)?;
    let mut entries = Vec::new();
    while let Some(info) = children
        .next_file(gio::Cancellable::NONE)
        .map_err(gio_error)?
    {
        entries.push(TrashEntry {
            item: children.child(&info),
            original_path: original_path_of(&info),
            deleted_at: info.deletion_date().map(|date| date.to_unix()),
        });
    }
    let item = newest_trashed(entries, original_path).ok_or(Error::not_found("trashed file"))?;
    if let Some(parent) = original_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| Error::internal(error.to_string()))?;
    }
    item.move_(
        &gio::File::for_path(original_path),
        gio::FileCopyFlags::NONE,
        gio::Cancellable::NONE,
        None,
    )
    .map_err(gio_error)
}

/// `trash::orig-path` holds the path's bytes, which need not be UTF-8.
fn original_path_of(info: &gio::FileInfo) -> Option<PathBuf> {
    // SAFETY: `info` is a live GFileInfo and the attribute name is NUL-terminated;
    // GIO returns null or a NUL-terminated string owned by `info`.
    let bytes = unsafe {
        gio::ffi::g_file_info_get_attribute_byte_string(
            info.to_glib_none().0,
            ORIGINAL_PATH.as_ptr(),
        )
    };
    if bytes.is_null() {
        return None;
    }
    // SAFETY: non-null, NUL-terminated, and copied out while `info` is borrowed.
    let bytes = unsafe { CStr::from_ptr(bytes) }.to_bytes();
    Some(PathBuf::from(OsStr::from_bytes(bytes)))
}

/// One item in the trash: a handle to it, where it was, and when it was
/// trashed in Unix seconds.
pub(super) struct TrashEntry<T> {
    pub(super) item: T,
    pub(super) original_path: Option<PathBuf>,
    pub(super) deleted_at: Option<i64>,
}

/// The entry trashed last from `original_path`; one without a date counts as
/// the oldest.
pub(super) fn newest_trashed<T>(entries: Vec<TrashEntry<T>>, original_path: &Path) -> Option<T> {
    entries
        .into_iter()
        .filter(|entry| entry.original_path.as_deref() == Some(original_path))
        .max_by_key(|entry| entry.deleted_at)
        .map(|entry| entry.item)
}
