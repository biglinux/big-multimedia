//! Bounded internal-state I/O and cooperating-writer transactions.
//! Parents must be application-owned directories. Advisory locks are not a
//! sandbox against arbitrary writers or directory replacement by the same UID.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Result after atomic publication; an error before publication remains Err.
#[derive(Debug)]
pub enum CommitOutcome {
    /// Contents and the containing directory were synchronized.
    Durable,
    /// The destination ALREADY contains the new bytes, but directory fsync failed.
    PublishedNotSynced(io::Error),
}
impl CommitOutcome {
    /// Compatibility conversion. The error explicitly says publication occurred.
    /// Transactional callers should match the outcome instead of blindly retrying.
    pub fn into_io_result(self) -> io::Result<()> {
        match self {
            Self::Durable => Ok(()),
            Self::PublishedNotSynced(error) => Err(io::Error::other(format!(
                "new contents published, durability not confirmed: {error}"
            ))),
        }
    }
}

/// Read one regular file through the same handle with a byte limit. Does not
/// follow a final symlink on Unix and does not block opening a FIFO.
/// Missing files return None; permission, corruption and excess-size errors do not.
pub fn read_regular_limited(path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds its read limit",
        ));
    }
    Ok(Some(bytes))
}

/// Nonblocking cooperating-writer lock. The lock inode is deliberately never
/// unlinked, otherwise another writer could lock a different inode at that name.
pub struct WriteLock {
    _file: File,
}
impl WriteLock {
    /// Acquire a lock next to the target; WouldBlock asks the caller to retry
    /// later, not to freeze GTK. Non-Unix targets fail before modifying state.
    pub fn try_acquire(target: &Path) -> io::Result<Self> {
        let mut name = target
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing target name"))?
            .to_os_string();
        name.push(".lock");
        let path = target.with_file_name(name);
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::OpenOptionsExt;
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)?;
            if !file.metadata()?.is_file() {
                return Err(io::Error::other("lock is not a regular file"));
            }
            // SAFETY: owned fd; LOCK_NB forbids waiting for another writer.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { _file: file })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "settings lock unsupported",
            ))
        }
    }
}

/// Synchronize the containing directory after publication (no rewrite).
pub fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(parent(path))?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory sync unsupported",
        ))
    }
}
fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Private staging file in the destination filesystem. Dropping it removes only
/// its private directory, never the final output. Keep it owned until the writer
/// has FINISHED, not merely after cancellation has been requested.
#[derive(Debug)]
pub struct StagedFile {
    directory: PathBuf,
    path: PathBuf,
}
impl StagedFile {
    /// Reserve a mode-0700 directory and mode-0600 regular file. The extension is
    /// inherited so tools that infer an encoder/container see the intended type.
    pub fn new(target: &Path) -> io::Result<Self> {
        let name = target
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing target name"))?;
        let parent = parent(target);
        let mut nonce = [0_u8; 16];
        // Linux/Unix desktop target; fail closed if OS randomness is unavailable.
        File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let hex: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let directory = parent.join(format!(".big-media-{hex}"));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        #[cfg(not(unix))]
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "private staging unsupported",
            ));
        }
        builder.create(&directory)?;
        let stage = Self {
            path: directory.join(name),
            directory,
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let _file = options.open(&stage.path)?;
        Ok(stage)
    }
    /// Path for the external writer. Do not expose it as a final user result.
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Publish without replacing an existing destination. Hard-link creation is
    /// atomic on a single filesystem and fails on EEXIST; there is no overwrite
    /// fallback. Validate media BEFORE this call. Result distinguishes durability.
    pub fn publish(&self, target: &Path) -> io::Result<CommitOutcome> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(&self.path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "output is empty or not a regular file",
            ));
        }
        file.sync_all()?;
        fs::hard_link(&self.path, target)?;
        Ok(match sync_parent(target) {
            Ok(()) => CommitOutcome::Durable,
            Err(error) => CommitOutcome::PublishedNotSynced(error),
        })
    }
}
impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        // Nonrecursive: never traverse a directory planted by another process.
        let _ = fs::remove_dir(&self.directory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "big-storage-review-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn staging_never_replaces_existing_output() {
        let dir = directory();
        let target = dir.join("out.png");
        fs::write(&target, b"original").unwrap();
        let stage = StagedFile::new(&target).unwrap();
        fs::write(stage.path(), b"new").unwrap();
        assert!(stage.publish(&target).is_err());
        drop(stage);
        assert_eq!(fs::read(&target).unwrap(), b"original");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn bounded_read_distinguishes_missing_and_oversize() {
        let dir = directory();
        let target = dir.join("state");
        assert!(read_regular_limited(&target, 3).unwrap().is_none());
        fs::write(&target, b"four").unwrap();
        assert!(read_regular_limited(&target, 3).is_err());
        assert_eq!(read_regular_limited(&target, 4).unwrap().unwrap(), b"four");
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn no_symlink_read_and_nonblocking_writer_lock() {
        let dir = directory();
        let target = dir.join("state");
        fs::write(&target, b"data").unwrap();
        std::os::unix::fs::symlink(&target, dir.join("link")).unwrap();
        assert!(read_regular_limited(&dir.join("link"), 10).is_err());
        let first = WriteLock::try_acquire(&target).unwrap();
        assert!(WriteLock::try_acquire(&target).is_err());
        drop(first);
        assert!(WriteLock::try_acquire(&target).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }
}
