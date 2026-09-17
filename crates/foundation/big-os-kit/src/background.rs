//! Bounded, serial executor for short persistence transactions. No GUI objects
//! cross this boundary. Admission is non-blocking; a full queue is an error, not
//! permission to silently drop a save. Jobs finish even if a widget disappears.
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, mpsc};
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
/// Number of admitted persistence jobs (including queued jobs).
pub fn pending() -> usize {
    ACTIVE.load(Ordering::Acquire)
}
/// Keeps normal application shutdown draining while an already bounded,
/// externally scheduled operation owns durable work. This does not spawn work;
/// callers must enforce their own admission limit before obtaining this guard.
pub struct OperationGuard {
    _ticket: Ticket,
}
/// Track an admitted scan/export running on another executor until its RAII
/// guard is dropped. Forced process termination remains outside this contract.
pub fn track_operation() -> OperationGuard {
    ACTIVE.fetch_add(1, Ordering::Release);
    OperationGuard { _ticket: Ticket }
}

struct Ticket;
impl Drop for Ticket {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::Release);
    }
}

type Job = Box<dyn FnOnce() + Send + 'static>;
const CAPACITY: usize = 64;
static WRITES: OnceLock<io::Result<mpsc::SyncSender<Job>>> = OnceLock::new();

/// Admit a short disk transaction without blocking the caller.
pub fn submit(job: impl FnOnce() + Send + 'static) -> io::Result<()> {
    let queue = WRITES
        .get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel::<Job>(CAPACITY);
            std::thread::Builder::new()
                .name("media-persistence".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        // In unwind builds one faulty task must not strand future saves.
                        // Release panic=abort remains process-fatal by product policy.
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                            log::error!("persistence task panicked");
                        }
                    }
                })?;
            Ok(tx)
        })
        .as_ref()
        .map_err(|error| io::Error::new(error.kind(), "persistence worker unavailable"))?;
    ACTIVE.fetch_add(1, Ordering::Release);
    let ticket = Ticket;
    queue
        .try_send(Box::new(move || {
            let _ticket = ticket;
            job();
        }))
        .map_err(|error| match error {
            mpsc::TrySendError::Full(_) => io::Error::new(
                io::ErrorKind::WouldBlock,
                "persistence queue is full; retry save",
            ),
            mpsc::TrySendError::Disconnected(_) => {
                io::Error::new(io::ErrorKind::BrokenPipe, "persistence worker stopped")
            }
        })
}

// Ephemeral plugin control files are latest-value registers, not preferences
// documents. One admitted task and one newest payload per path prevent slider
// motion from filling the global disk queue. No filesystem work runs in submit.
use crate::deferred::{Phase, Status};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
struct LiveState {
    bytes: Vec<u8>,
    revision: u64,
    published: u64,
    durable: u64,
    queued: bool,
    needs_sync: bool,
    phase: Phase,
}
struct LiveFile {
    path: PathBuf,
    state: Mutex<LiveState>,
}
static LIVE: OnceLock<Mutex<BTreeMap<PathBuf, Arc<LiveFile>>>> = OnceLock::new();

/// Replace a small internal live-control value asynchronously, coalescing older
/// pending values. This deliberately overwrites an existing internal register;
/// use `deferred::Document` for compare-and-save preferences, or StageFile for
/// user documents. At most eight paths and 4096 bytes per path are retained.
pub fn write_private(path: PathBuf, bytes: Vec<u8>) -> io::Result<()> {
    if !path.is_absolute() || bytes.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid live-control path or payload",
        ));
    }
    let file = {
        let mut files = LIVE
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !files.contains_key(&path) && files.len() >= 8 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "live-control register limit reached",
            ));
        }
        files
            .entry(path.clone())
            .or_insert_with(|| {
                Arc::new(LiveFile {
                    path,
                    state: Mutex::new(LiveState {
                        bytes: Vec::new(),
                        revision: 0,
                        published: 0,
                        durable: 0,
                        queued: false,
                        needs_sync: false,
                        phase: Phase::Saved,
                    }),
                })
            })
            .clone()
    };
    {
        let mut state = file.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| io::Error::other("live-control generation exhausted"))?;
        state.bytes = bytes;
    }
    enqueue_live(file)
}

/// Last acknowledged live-control status; no path/content is exposed in Status.
pub fn live_status(path: &Path) -> Option<Status> {
    let file = LIVE
        .get()?
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(path)
        .cloned()?;
    let s = file.state.lock().unwrap_or_else(PoisonError::into_inner);
    Some(Status {
        phase: s.phase,
        revision: s.revision,
        durable_revision: s.durable,
    })
}

/// Retry the latest retained internal control value after admission or I/O fails.
pub fn retry_live(path: &Path) -> io::Result<()> {
    let file = LIVE
        .get()
        .and_then(|files| {
            files
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(path)
                .cloned()
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no live-control update pending"))?;
    enqueue_live(file)
}
fn enqueue_live(file: Arc<LiveFile>) -> io::Result<()> {
    {
        let mut s = file.state.lock().unwrap_or_else(PoisonError::into_inner);
        if s.queued || (s.revision == s.durable && !s.needs_sync) {
            return Ok(());
        }
        s.queued = true;
        s.phase = Phase::Pending;
    }
    let owner = file.clone();
    let result = submit(move || {
        let (bytes, revision, sync_only) = {
            let mut s = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
            s.phase = Phase::Saving;
            (
                s.bytes.clone(),
                s.revision,
                s.needs_sync && s.revision == s.published,
            )
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> io::Result<crate::storage::CommitOutcome> {
                let parent = owner
                    .path
                    .parent()
                    .ok_or_else(|| io::Error::other("invalid control directory"))?;
                std::fs::create_dir_all(parent)?;
                let _lock = crate::storage::WriteLock::try_acquire(&owner.path)?;
                if sync_only {
                    crate::storage::sync_parent(&owner.path)?;
                    Ok(crate::storage::CommitOutcome::Durable)
                } else {
                    crate::storage::atomic_write_private_receipt(&owner.path, &bytes)
                }
            },
        ))
        .unwrap_or_else(|_| Err(io::Error::other("live-control task panicked")));
        let again = {
            let mut s = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
            s.queued = false;
            match outcome {
                Ok(crate::storage::CommitOutcome::Durable) => {
                    s.published = revision;
                    s.durable = revision;
                    s.needs_sync = false;
                    s.phase = if s.revision == revision {
                        Phase::Saved
                    } else {
                        Phase::Pending
                    };
                    s.revision != revision
                }
                Ok(crate::storage::CommitOutcome::PublishedNotSynced(_)) => {
                    s.published = revision;
                    s.needs_sync = true;
                    s.phase = Phase::DurabilityPending;
                    false
                }
                Err(_) => {
                    s.phase = if sync_only {
                        Phase::DurabilityPending
                    } else {
                        Phase::Failed
                    };
                    false
                }
            }
        };
        if again {
            let _ = enqueue_live(owner);
        }
    });
    if result.is_err() {
        let mut s = file.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.queued = false;
        s.phase = Phase::Failed;
    }
    result
}

// Account providers can wait for an unlock prompt; never let such a prompt
// occupy the short disk-write lane. Both lanes participate in close draining.
static SERVICES: OnceLock<io::Result<mpsc::SyncSender<Job>>> = OnceLock::new();
/// Admit a bounded account/service operation on a separate serial lane.
/// This is for infrequent account work, not continuous playback or HTTP polling.
pub fn submit_service(job: impl FnOnce() + Send + 'static) -> io::Result<()> {
    let queue = SERVICES
        .get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel::<Job>(8);
            std::thread::Builder::new()
                .name("media-account-service".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                            log::error!("account operation panicked; inspect recovery journal");
                        }
                    }
                })?;
            Ok(tx)
        })
        .as_ref()
        .map_err(|e| io::Error::new(e.kind(), "account worker unavailable"))?;
    ACTIVE.fetch_add(1, Ordering::Release);
    let ticket = Ticket;
    queue
        .try_send(Box::new(move || {
            let _ticket = ticket;
            job();
        }))
        .map_err(|error| match error {
            mpsc::TrySendError::Full(_) => {
                io::Error::new(io::ErrorKind::WouldBlock, "account queue is full")
            }
            mpsc::TrySendError::Disconnected(_) => {
                io::Error::new(io::ErrorKind::BrokenPipe, "account worker stopped")
            }
        })
}
