// SPDX-License-Identifier: MIT

//! Atomic file persistence primitives for BigLinux apps.
//!
//! [`atomic_write`](crate::storage::atomic_write) and
//! [`atomic_write_private_receipt`](crate::storage::atomic_write_private_receipt) write through a
//! `.big-state.tmp.<pid>.<seq>` sibling file and then rename into place.
//! `rename(2)` is atomic on the same filesystem; partial writes from a crash
//! are bound to the temp file and visible only after a successful rename.

use std::fs;
use std::io::Write;

mod transaction;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
pub use transaction::{CommitOutcome, StagedFile, WriteLock, read_regular_limited, sync_parent};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn atomic_sibling_temp_path(target: &Path) -> PathBuf {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(".big-state.tmp.{pid}.{seq}"))
}

/// Atomically write `bytes` to `path`: write a uniquely-named sibling temp file,
/// `fsync` it, then rename it over the target. A crash mid-write leaves the
/// previous contents intact (a plain `fs::write` truncates first). The temp file
/// is created in the target's parent directory so the rename stays within one
/// filesystem (a cross-filesystem rename fails). Creates the parent dir if
/// missing.
///
/// GTK-free and blocking — offload it (e.g. via
/// `big_relm4_components::file_io::write_bytes_async`) so a slow/network
/// filesystem can't stall the UI thread.
///
/// NOTE: the rename installs a *new* inode, so the result carries the temp
/// file's permissions/owner (default umask), not the previous file's, and any
/// hard links to the old inode are left pointing at the old contents. Right for
/// config/state and fresh files; for editing a user document whose mode must be
/// preserved, use a document-specific atomic save policy preserving mode/ACLs.
/// Blind in-place writes are not recommended because they lose crash atomicity.
///
/// # Errors
/// Returns the underlying [`std::io::Error`] from the dir create, temp write,
/// `fsync`, or rename.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    atomic_replace(path, false, |file| file.write_all(bytes))
}

// Remove partial files on every error, including serialization and fsync errors.
struct TempFileCleanup(Option<PathBuf>);
impl Drop for TempFileCleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

fn create_sibling_file(path: &Path, private: bool) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    // O_EXCL: never truncate or follow an existing (including dangling) symlink.
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    if private {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "private atomic files are not implemented on this platform",
        ));
    }
    options.open(path)
}

fn atomic_replace_receipt(
    path: &Path,
    private: bool,
    write: impl FnOnce(&mut fs::File) -> std::io::Result<()>,
) -> std::io::Result<CommitOutcome> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = atomic_sibling_temp_path(path);
    // Do not arm cleanup before exclusive creation succeeds: an existing
    // attacker-owned path must neither be followed nor removed by this code.
    let mut file = create_sibling_file(&temporary, private)?;
    let mut cleanup = TempFileCleanup(Some(temporary.clone()));
    write(&mut file)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    cleanup.0 = None; // the old temporary name no longer belongs to us
    // Persist the directory entry as well as the file content on Unix.
    Ok(match sync_parent(path) {
        Ok(()) => CommitOutcome::Durable,
        Err(error) => CommitOutcome::PublishedNotSynced(error),
    })
}

fn atomic_replace(
    path: &Path,
    private: bool,
    write: impl FnOnce(&mut fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    atomic_replace_receipt(path, private, write)?.into_io_result()
}

/// Publish private internal state with an explicit publication/durability receipt.
/// Err means publication did not happen; PublishedNotSynced means it DID happen.
pub fn atomic_write_private_receipt(path: &Path, bytes: &[u8]) -> std::io::Result<CommitOutcome> {
    atomic_replace_receipt(path, true, |file| file.write_all(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage_test_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        p.push(format!("big-app-kit-storage-test-{pid}-{seq}-{name}.json"));
        p
    }

    #[test]
    fn atomic_write_creates_file_with_exact_bytes() {
        let path = storage_test_path("atomic-create");
        atomic_write(&path, b"hello\0world").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello\0world");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn atomic_write_overwrites_and_leaves_no_temp() {
        let path = storage_test_path("atomic-overwrite");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second-longer").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second-longer");
        // The sibling temp file (`.<name>.tmp.<pid>.<seq>`) must be renamed away,
        // not left behind.
        let parent = path.parent().unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let stray: Vec<_> = fs::read_dir(parent)
            .unwrap()
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.starts_with(&format!(".{name}.tmp."))
            })
            .collect();
        assert!(stray.is_empty(), "temp file left behind: {stray:?}");
        let _ = fs::remove_file(&path);
    }
}

#[cfg(test)]
mod review_atomic_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn exclusive_creation_never_follows_a_symlink() {
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!(
            "big-private-{}-{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&base).unwrap();
        let victim = base.join("victim");
        fs::write(&victim, b"keep").unwrap();
        let link = base.join("temp");
        symlink(&victim, &link).unwrap();
        assert!(create_sibling_file(&link, true).is_err());
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn failed_write_preserves_old_content_and_removes_partial_file() {
        let base = std::env::temp_dir().join(format!(
            "big-rollback-{}-{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&base).unwrap();
        let path = base.join("state");
        fs::write(&path, b"old").unwrap();
        let result: std::io::Result<()> = atomic_replace(&path, false, |file| {
            file.write_all(b"partial")?;
            Err(std::io::Error::other("injected write failure"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(fs::read_dir(&base).unwrap().count(), 1);
        fs::remove_dir_all(base).unwrap();
    }
}
