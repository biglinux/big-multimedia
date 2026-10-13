use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::i18n::i18n;
use big_app_kit::subprocess::BigSubprocessSpec;

/// Shells out to `gio trash`. `Err` carries the stderr message verbatim.
pub fn trash_file(path: &str) -> Result<()> {
    let output = BigSubprocessSpec::builder()
        .program("gio")
        // A file name starting with `-` must not read as a flag.
        .args(["trash", "--", path])
        .build()
        .run()
        .map_err(|e| Error::internal(e.to_string()))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(Error::internal(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

/// Looks up the trashed file by its original path, then asks gio to restore it.
pub fn restore_from_trash(original_path: &str) -> Result<()> {
    let list_output = BigSubprocessSpec::builder()
        .program("gio")
        .args(["trash", "--list"])
        .build()
        .run()
        .map_err(|e| Error::internal(e.to_string()))?;

    if !list_output.status.success() {
        return Err(Error::internal("gio trash --list failed"));
    }

    let listing = String::from_utf8_lossy(&list_output.stdout);
    let uri =
        trashed_uri(&listing, original_path).ok_or_else(|| Error::not_found("trashed file"))?;
    let restore = BigSubprocessSpec::builder()
        .program("gio")
        .args(["trash", "--restore", uri])
        .build()
        .run()
        .map_err(|e| Error::internal(e.to_string()))?;
    if restore.status.success() {
        Ok(())
    } else {
        Err(Error::internal(
            String::from_utf8_lossy(&restore.stderr).to_string(),
        ))
    }
}

/// The `trash://` URI whose original location is exactly `original_path`, from
/// `gio trash --list` output: one `URI<TAB>original path` line per item.
pub(super) fn trashed_uri<'a>(listing: &'a str, original_path: &str) -> Option<&'a str> {
    listing.lines().find_map(|line| {
        let (uri, path) = line.split_once('\t')?;
        (path == original_path && uri.starts_with("trash:///")).then_some(uri)
    })
}

pub fn folder_display_name(path: &str) -> String {
    big_app_kit::files::folder_display_name(path)
}

/// Returns `(display_name, full_path)` per ancestor up to `~` or `/`.
pub fn breadcrumb_segments(folder: &str) -> Vec<(String, String)> {
    let path = Path::new(folder);
    let home = big_os_kit::xdg_dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home"));
    let mut segments = Vec::new();
    let mut current = path.to_path_buf();

    loop {
        let name = if current == home {
            i18n("Home")
        } else {
            current.file_name().map_or_else(
                || "/".to_string(),
                |name| name.to_string_lossy().to_string(),
            )
        };
        segments.push((name, current.to_string_lossy().to_string()));

        if current == home || current.parent().is_none() || current == Path::new("/") {
            break;
        }
        current = current
            .parent()
            .expect("guarded by is_none check above")
            .to_path_buf();
    }

    segments.reverse();
    segments
}
