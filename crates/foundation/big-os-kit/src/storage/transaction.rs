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

/// Open a regular file for reading. Does not follow a final symlink on Unix
/// and does not block opening a FIFO.
///
/// # Errors
///
/// The open error (`NotFound` for a missing file, `ELOOP` for a final
/// symlink), or `InvalidData` when the path is not a regular file.
pub fn open_regular_read(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    {
        use std::os::unix::fs::OpenOptionsExt;
        #[expect(clippy::disallowed_methods, reason = "the owner of no-follow reads")]
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected a regular file",
        ));
    }
    Ok(file)
}

/// Read one regular file through the same handle with a byte limit, opened
/// as [`open_regular_read`] does.
/// Missing files return None; permission, corruption and excess-size errors do not.
pub fn read_regular_limited(path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let file = match open_regular_read(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
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
    /// later, not to freeze GTK.
    pub fn try_acquire(target: &Path) -> io::Result<Self> {
        let mut name = target
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing target name"))?
            .to_os_string();
        name.push(".lock");
        let path = target.with_file_name(name);
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::OpenOptionsExt;
            #[expect(
                clippy::disallowed_methods,
                reason = "creates the lock file, which a read-only open cannot"
            )]
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
    }
}

/// Synchronize the containing directory after publication (no rewrite).
pub fn sync_parent(path: &Path) -> io::Result<()> {
    File::open(parent(path))?.sync_all()
}
fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Leading name of every staging entry next to user files.
pub(super) const STAGING_PREFIX: &str = ".big-media-";

/// Name prefix, carrying this machine's tag and this process's PID, for
/// staging next to user files; [`remove_stale_staging`] removes such entries
/// once the process has exited.
pub fn staging_prefix() -> String {
    format!(
        "{STAGING_PREFIX}{}-{}-",
        machine_tag().unwrap_or("unknown"),
        std::process::id()
    )
}

/// `m` and the first 8 hex digits of `/etc/machine-id`: a folder shared by
/// two machines holds staging whose PID means nothing to the other one's
/// process table. `None` when the id cannot be read; such staging is never
/// removed.
fn machine_tag() -> Option<&'static str> {
    static TAG: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    TAG.get_or_init(|| {
        let id = read_regular_limited(Path::new("/etc/machine-id"), 64).ok()??;
        let hex = id.get(..8)?;
        hex.iter()
            .all(u8::is_ascii_hexdigit)
            .then(|| format!("m{}", String::from_utf8_lossy(hex).to_ascii_lowercase()))
    })
    .as_deref()
}

/// Best-effort removal of the staging entries in `directory` that a killed
/// process of the current user on this machine left behind, once per
/// directory per process: callers stage many files into one folder. A live
/// owner keeps its entries.
pub fn remove_stale_staging(directory: &Path) {
    use std::os::unix::fs::MetadataExt;
    static SWEPT: std::sync::Mutex<std::collections::BTreeSet<PathBuf>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    let Some(tag) = machine_tag() else {
        return;
    };
    if !SWEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(directory.to_owned())
    {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    // SAFETY: geteuid has no preconditions and cannot fail.
    let uid = unsafe { libc::geteuid() };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|name| staging_owner(name, tag)) else {
            continue;
        };
        // SAFETY: signal 0 only checks that the process exists; pid is positive.
        if unsafe { libc::kill(pid, 0) } == 0
            || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        {
            continue;
        }
        // DirEntry::metadata does not follow symlinks.
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.uid() != uid {
            continue;
        }
        let path = entry.path();
        if metadata.is_dir() {
            if let Ok(staged) = fs::read_dir(&path) {
                for file in staged.flatten() {
                    let _ = fs::remove_file(file.path());
                }
            }
            let _ = fs::remove_dir(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
}

/// The PID in a staging name made on the machine tagged `tag`.
fn staging_owner(name: &str, tag: &str) -> Option<i32> {
    let rest = name.strip_prefix(STAGING_PREFIX)?.strip_prefix(tag)?;
    let (pid, _) = rest.strip_prefix('-')?.split_once('-')?;
    pid.parse::<i32>().ok().filter(|pid| *pid > 0)
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
    /// Reserve a mode-0700 directory holding a regular file created with the
    /// umask's mode, which the published file keeps. The extension is inherited
    /// so tools that infer an encoder/container see the intended type.
    pub fn new(target: &Path) -> io::Result<Self> {
        let name = target
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing target name"))?;
        let parent = parent(target);
        remove_stale_staging(parent);
        let hex = crate::random::random_hex_id()?;
        let directory = parent.join(format!("{}{hex}", staging_prefix()));
        let mut builder = fs::DirBuilder::new();
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        let stage = Self {
            path: directory.join(name),
            directory,
        };
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage.path)?;
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
    /// An empty file is refused as a failed writer's output.
    pub fn publish(&self, target: &Path) -> io::Result<CommitOutcome> {
        self.place(target, false)
    }
    /// [`Self::publish`] for a copy of a file that exists elsewhere, such as a
    /// move across filesystems, where an empty file is a complete copy.
    pub fn publish_copy(&self, target: &Path) -> io::Result<CommitOutcome> {
        self.place(target, true)
    }
    fn place(&self, target: &Path, empty_allowed: bool) -> io::Result<CommitOutcome> {
        let file = open_regular_read(&self.path)?;
        if file.metadata()?.len() == 0 && !empty_allowed {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "output is empty",
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
    #[expect(clippy::disallowed_methods, reason = "the owner of no-replace renames")]
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
    let mut input = File::open(stage)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let copied = io::copy(&mut input, &mut output).and_then(|_| output.sync_all());
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
    #[test]
    fn only_copies_publish_empty_files() {
        let dir = directory();
        let target = dir.join("out.png");
        let stage = StagedFile::new(&target).unwrap();
        let error = stage.publish(&target).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!target.exists());
        stage
            .publish_copy(&target)
            .unwrap()
            .into_io_result()
            .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"");
        drop(stage);
        fs::remove_dir_all(dir).unwrap();
    }
    /// Staging of an exited process of this machine is removed by the next
    /// staging in that directory; a live process keeps its own, and another
    /// machine's is never judged by this process table. No PID reaches `i32::MAX`.
    #[test]
    fn staging_removes_dead_process_leftovers() {
        let dir = directory();
        let tag = machine_tag().expect("this machine has an id");
        let dead_directory = dir.join(format!(".big-media-{tag}-{}-0a1b", i32::MAX));
        fs::create_dir(&dead_directory).unwrap();
        fs::write(dead_directory.join("out.png"), b"partial").unwrap();
        let dead_file = dir.join(format!(".big-media-{tag}-{}-x7Yz.flac", i32::MAX));
        fs::write(&dead_file, b"partial").unwrap();
        let live_directory = dir.join(format!("{}0a1b", staging_prefix()));
        fs::create_dir(&live_directory).unwrap();
        let other_machine = dir.join(format!(".big-media-m00000000-{}-0a1b", i32::MAX));
        fs::create_dir(&other_machine).unwrap();
        let stage = StagedFile::new(&dir.join("out.png")).unwrap();
        assert!(!dead_directory.exists());
        assert!(!dead_file.exists());
        assert!(live_directory.exists());
        assert!(other_machine.exists());
        assert!(stage.path().exists());
        drop(stage);
        fs::remove_dir_all(dir).unwrap();
    }
    /// A published file is as readable as any file the user creates, so media
    /// servers and other accounts can read downloads and exports.
    #[test]
    fn published_file_follows_umask() {
        use std::os::unix::fs::PermissionsExt;
        let dir = directory();
        let reference = dir.join("reference");
        File::create(&reference).unwrap();
        let target = dir.join("out.png");
        let stage = StagedFile::new(&target).unwrap();
        fs::write(stage.path(), b"new").unwrap();
        stage.publish(&target).unwrap().into_io_result().unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&target), mode(&reference));
        fs::remove_dir_all(dir).unwrap();
    }
    fn refuse_link(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
    fn rename_unsupported(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EINVAL))
    }
    #[test]
    fn unreadable_stage_leaves_no_target() {
        let dir = directory();
        let target = dir.join("out.png");
        let missing = dir.join("stage");
        place_no_replace(&missing, &target, refuse_link, rename_unsupported).unwrap_err();
        assert!(!target.exists());
        fs::remove_dir_all(dir).unwrap();
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
    /// The `statfs` magic number of the filesystem holding `path`.
    fn filesystem_type(path: &Path) -> libc::c_long {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: c_path is a NUL-terminated path bound for the whole call,
        // and stat is writable storage for one statfs.
        let status = unsafe { libc::statfs(c_path.as_ptr(), stat.as_mut_ptr()) };
        let error = io::Error::last_os_error();
        assert_eq!(status, 0, "statfs {}: {error}", path.display());
        // SAFETY: statfs returned 0, so it filled stat.
        unsafe { stat.assume_init() }.f_type
    }
    /// The real filesystem counterpart of `publish_falls_back_without_replacing`.
    #[test]
    #[ignore = "needs a FAT/exFAT directory in BIG_OS_KIT_FAT_DIR"]
    fn fat_publish_without_hard_links() {
        // From linux/magic.h; libc names only MSDOS_SUPER_MAGIC.
        const EXFAT_SUPER_MAGIC: libc::c_long = 0x2011_BAB0;
        let root = PathBuf::from(
            std::env::var_os("BIG_OS_KIT_FAT_DIR")
                .expect("BIG_OS_KIT_FAT_DIR must name a directory on a vfat or exfat mount"),
        );
        let filesystem = filesystem_type(&root);
        assert!(
            matches!(filesystem, libc::MSDOS_SUPER_MAGIC | EXFAT_SUPER_MAGIC),
            "{} is not on vfat or exfat (f_type {filesystem:#x})",
            root.display()
        );

        let dir = root.join(format!("big-os-kit-fat-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let probe = dir.join("probe");
        fs::write(&probe, b"").unwrap();
        let refused = fs::hard_link(&probe, dir.join("probe-link")).unwrap_err();
        assert_eq!(refused.raw_os_error(), Some(libc::EPERM), "{refused}");
        fs::remove_file(probe).unwrap();

        let target = dir.join("out.png");
        let stage = StagedFile::new(&target).unwrap();
        fs::write(stage.path(), b"new").unwrap();
        stage.publish(&target).unwrap().into_io_result().unwrap();
        drop(stage);
        let empty_target = dir.join("empty.png");
        let stage = StagedFile::new(&empty_target).unwrap();
        stage
            .publish_copy(&empty_target)
            .unwrap()
            .into_io_result()
            .unwrap();
        drop(stage);
        let stage = StagedFile::new(&target).unwrap();
        fs::write(stage.path(), b"newer").unwrap();
        let error = stage.publish(&target).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{error}");
        drop(stage);

        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(fs::read(&empty_target).unwrap(), b"");
        let mut names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["empty.png", "out.png"], "staging left behind");
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
    #[test]
    fn regular_open_refuses_special_files() {
        use std::os::unix::ffi::OsStrExt;
        let dir = directory();
        let fifo = dir.join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `name` is a live NUL-terminated path for the whole call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let kind = |path: &Path| open_regular_read(path).unwrap_err().kind();
        assert_eq!(kind(&fifo), io::ErrorKind::InvalidData);
        assert_eq!(kind(&dir), io::ErrorKind::InvalidData);
        assert_eq!(kind(&dir.join("missing")), io::ErrorKind::NotFound);
        fs::remove_dir_all(dir).unwrap();
    }
}
