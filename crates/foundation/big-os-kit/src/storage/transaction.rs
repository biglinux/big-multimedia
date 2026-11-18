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
    /// The destination holds the new bytes, but the directory fsync failed.
    PublishedNotSynced(io::Error),
}
impl CommitOutcome {
    /// Collapse into `io::Result`; the error says publication occurred.
    /// Transactional callers match the outcome instead of retrying.
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

/// Nonblocking cooperating-writer lock. The lock inode is never
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
            // A child forked by another thread shares the lock until it execs,
            // a window of microseconds; a few short tries outlast it, while a
            // real writer still holds the lock after them.
            for attempt in 0.. {
                // SAFETY: owned fd; LOCK_NB forbids waiting for another writer.
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    break;
                }
                let error = io::Error::last_os_error();
                if attempt == 4 || error.kind() != io::ErrorKind::WouldBlock {
                    return Err(error);
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
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
/// has exited, not merely until cancellation was requested.
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
    /// Publish without replacing an existing destination, which fails with
    /// `AlreadyExists`. Atomic unless the filesystem supports neither hard links
    /// nor `RENAME_NOREPLACE`, where the bytes are copied into a newly created
    /// file. Validate the media first. The outcome says whether it is durable.
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
        place_no_replace(&self.path, target, hard_link, rename_no_replace)?;
        Ok(match sync_parent(target) {
            Ok(()) => CommitOutcome::Durable,
            Err(error) => CommitOutcome::PublishedNotSynced(error),
        })
    }
}
fn hard_link(stage: &Path, target: &Path) -> io::Result<()> {
    fs::hard_link(stage, target)
}

/// `rename` that fails with `AlreadyExists` instead of replacing `dest`.
/// Filesystems without `RENAME_NOREPLACE` (some FUSE and NFS mounts) fail with
/// `EINVAL`; the caller chooses the fallback.
pub fn rename_no_replace(src: &Path, dest: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let src = std::ffi::CString::new(src.as_os_str().as_bytes())?;
    let dest = std::ffi::CString::new(dest.as_os_str().as_bytes())?;
    // SAFETY: both pointers come from CStrings bound for the whole call, and
    // renameat2 only reads them.
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            src.as_ptr(),
            libc::AT_FDCWD,
            dest.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

type MoveStep = fn(&Path, &Path) -> io::Result<()>;

/// Move `stage` to the absent `target`: a hard link, then a no-replace rename
/// where links are refused (FAT, exFAT, some FUSE and SMB mounts), then an
/// exclusive create and copy where that rename is unsupported.
fn place_no_replace(
    stage: &Path,
    target: &Path,
    link: MoveStep,
    rename: MoveStep,
) -> io::Result<()> {
    let refused = match link(stage, target) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    if !matches!(
        refused.raw_os_error(),
        Some(libc::EPERM | libc::EOPNOTSUPP | libc::EXDEV | libc::EMLINK | libc::ENOSYS)
    ) {
        return Err(refused);
    }
    match rename(stage, target) {
        Err(error) if matches!(error.raw_os_error(), Some(libc::EINVAL | libc::ENOSYS)) => {}
        result => return result,
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let copied = io::copy(&mut File::open(stage)?, &mut output).and_then(|_| output.sync_all());
    if copied.is_err() {
        // create_new made this file, so it holds only our partial copy.
        let _ = fs::remove_file(target);
    }
    copied
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
            "big-storage-test-{}-{}",
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
    fn refuse_link(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
    fn rename_unsupported(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EINVAL))
    }
    /// Filesystems without hard links (FAT, exFAT, some FUSE and SMB mounts)
    /// fall back to a no-replace rename, then to an exclusive create and copy.
    #[test]
    fn publish_falls_back_without_replacing() {
        let steps: [(MoveStep, MoveStep); 3] = [
            (hard_link, rename_no_replace),
            (refuse_link, rename_no_replace),
            (refuse_link, rename_unsupported),
        ];
        for (index, (link, rename)) in steps.into_iter().enumerate() {
            let dir = directory();
            let stage = dir.join("stage");
            let target = dir.join("out.png");
            fs::write(&stage, b"new").unwrap();
            place_no_replace(&stage, &target, link, rename)
                .unwrap_or_else(|error| panic!("step chain {index}: {error}"));
            assert_eq!(fs::read(&target).unwrap(), b"new", "step chain {index}");

            fs::write(&target, b"original").unwrap();
            let second_stage = dir.join("second-stage");
            fs::write(&second_stage, b"newer").unwrap();
            let error = place_no_replace(&second_stage, &target, link, rename).unwrap_err();
            assert_eq!(
                error.kind(),
                io::ErrorKind::AlreadyExists,
                "step chain {index}"
            );
            assert_eq!(
                fs::read(&target).unwrap(),
                b"original",
                "step chain {index}"
            );
            fs::remove_dir_all(dir).unwrap();
        }
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
