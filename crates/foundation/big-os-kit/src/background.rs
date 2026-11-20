//! Bounded, serial executor for short persistence transactions. No GUI objects
//! cross this boundary. Admission is non-blocking; a full queue is an error, not
//! permission to silently drop a save. Jobs finish even if a widget disappears.
use std::io;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
/// Number of admitted persistence jobs (including queued jobs).
pub fn pending() -> usize {
    ACTIVE.load(Ordering::Acquire)
}

type OnChange = Box<dyn Fn() + Send + Sync>;
static WATCHERS: Mutex<Vec<(u64, OnChange)>> = Mutex::new(Vec::new());
static NEXT_WATCH: AtomicU64 = AtomicU64::new(0);

/// Registration returned by [`watch`]; dropping it removes the callback.
#[must_use = "dropping the watch stops its notifications"]
pub struct Watch {
    id: u64,
}

impl Drop for Watch {
    fn drop(&mut self) {
        WATCHERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(id, _)| *id != self.id);
    }
}

/// Call `on_change` after a job is admitted or finishes and after [`changed`],
/// on the thread that did it, until the returned [`Watch`] drops. It runs under
/// the watcher lock, so it must neither block nor call into this module.
pub fn watch(on_change: impl Fn() + Send + Sync + 'static) -> Watch {
    let id = NEXT_WATCH.fetch_add(1, Ordering::Relaxed);
    WATCHERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, Box::new(on_change)));
    Watch { id }
}

/// Wake every [`watch`] after a save owner changed its status outside a job.
pub fn changed() {
    for (_, on_change) in WATCHERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
    {
        on_change();
    }
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
        changed();
    }
}

type Job = Box<dyn FnOnce() + Send + 'static>;
type Lane = OnceLock<io::Result<mpsc::SyncSender<Job>>>;
static WRITES: Lane = OnceLock::new();
// A keyring unlock prompt or a library check can wait long; never let one
// occupy the short disk-write lane. Both lanes participate in close draining.
static SERVICES: Lane = OnceLock::new();

/// Admit a short disk transaction without blocking the caller.
pub fn submit(job: impl FnOnce() + Send + 'static) -> io::Result<()> {
    admit(&WRITES, "media-persistence", 64, Box::new(job))
}

/// Admit a bounded operation that may wait long, such as keyring access or a
/// library check, on a separate serial lane. Not for continuous playback or
/// HTTP polling.
pub fn submit_service(job: impl FnOnce() + Send + 'static) -> io::Result<()> {
    admit(&SERVICES, "media-long-tasks", 8, Box::new(job))
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
        .inspect(|()| changed())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn watch_hears_admission_and_completion_until_dropped() {
        let (heard, events) = mpsc::channel();
        let watch = watch(move || {
            let _ = heard.send(());
        });
        let (release, blocked) = mpsc::channel::<()>();
        submit(move || {
            let _ = blocked.recv();
        })
        .unwrap();
        let wait = Duration::from_secs(5);
        events.recv_timeout(wait).expect("admission");
        release.send(()).unwrap();
        events.recv_timeout(wait).expect("completion");
        drop(watch);
        while events.try_recv().is_ok() {}
        assert_eq!(
            events.try_recv(),
            Err(mpsc::TryRecvError::Disconnected),
            "the callback is released with its watch"
        );
    }
}
