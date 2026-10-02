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
type Lane = OnceLock<io::Result<mpsc::SyncSender<Job>>>;
static WRITES: Lane = OnceLock::new();
// Account providers can wait for an unlock prompt; never let such a prompt
// occupy the short disk-write lane. Both lanes participate in close draining.
static SERVICES: Lane = OnceLock::new();

/// Admit a short disk transaction without blocking the caller.
pub fn submit(job: impl FnOnce() + Send + 'static) -> io::Result<()> {
    admit(&WRITES, "media-persistence", 64, Box::new(job))
}

/// Admit a bounded account/service operation on a separate serial lane.
/// This is for infrequent account work, not continuous playback or HTTP polling.
pub fn submit_service(job: impl FnOnce() + Send + 'static) -> io::Result<()> {
    admit(&SERVICES, "media-account-service", 8, Box::new(job))
}

/// Queue `job` on the serial worker `name`, started on first use.
fn admit(lane: &'static Lane, name: &'static str, capacity: usize, job: Job) -> io::Result<()> {
    let queue = lane
        .get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel::<Job>(capacity);
            std::thread::Builder::new()
                .name(name.into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        // In unwind builds one faulty task must not strand future saves.
                        // Release panic=abort remains process-fatal by product policy.
                        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                            log::error!("{name} task panicked");
                        }
                    }
                })?;
            Ok(tx)
        })
        .as_ref()
        .map_err(|error| io::Error::new(error.kind(), format!("{name} worker unavailable")))?;
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
                format!("{name} queue is full; retry"),
            ),
            mpsc::TrySendError::Disconnected(_) => {
                io::Error::new(io::ErrorKind::BrokenPipe, format!("{name} worker stopped"))
            }
        })
}
