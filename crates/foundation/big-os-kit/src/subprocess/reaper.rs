//! Bounded process-wide child collector. Reserve a permit before spawning.
//! Never creates one thread per child, never calls blocking Child::wait().
use std::io;
use std::process::Child;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

const MAX_CHILDREN: usize = 128;
struct State {
    reserved: usize,
    children: Vec<Child>,
}
struct Collector {
    state: Mutex<State>,
    wake: Condvar,
}
static COLLECTOR: Mutex<Option<Arc<Collector>>> = Mutex::new(None);

pub(crate) struct Permit {
    collector: Arc<Collector>,
    active: bool,
}
impl Permit {
    pub(crate) fn reserve() -> io::Result<Self> {
        let collector = {
            let mut slot = COLLECTOR.lock().unwrap_or_else(PoisonError::into_inner);
            if slot.is_none() {
                let collector = Arc::new(Collector {
                    state: Mutex::new(State {
                        reserved: 0,
                        children: Vec::with_capacity(MAX_CHILDREN),
                    }),
                    wake: Condvar::new(),
                });
                let worker = Arc::clone(&collector);
                // Failure propagates before a child exists. No fallible spawn
                // or allocation is needed on an error-cleanup path later.
                std::thread::Builder::new()
                    .name("big-child-reaper".into())
                    .spawn(move || worker.run())?;
                *slot = Some(collector);
            }
            Arc::clone(slot.as_ref().expect("collector initialized above"))
        };
        {
            let mut state = collector
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if state.reserved == MAX_CHILDREN {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "too many managed subprocesses",
                ));
            }
            state.reserved += 1;
        }
        Ok(Self {
            collector,
            active: true,
        })
    }

    pub(crate) fn defer(mut self, child: Child) {
        let mut state = self
            .collector
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // The pre-reserved slot bounds the vector; this path cannot fail due to
        // thread creation. The collector now owns the slot and the Child.
        state.children.push(child);
        self.active = false;
        self.collector.wake.notify_one();
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        if self.active {
            let mut state = self
                .collector
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.reserved -= 1;
        }
    }
}
impl Collector {
    fn run(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if state.children.is_empty() {
                state = self
                    .wake
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
                continue;
            }
            let before = state.children.len();
            state.children.retain_mut(|child| match child.try_wait() {
                Ok(Some(_)) => false,
                // ECHILD means an external SIGCHLD handler already collected
                // it. Never signal a recycled PID. Other errors remain retryable.
                #[cfg(unix)]
                Err(error) if error.raw_os_error() == Some(libc::ECHILD) => false,
                _ => true,
            });
            let collected = before - state.children.len();
            state.reserved -= collected;
            state = self
                .wake
                .wait_timeout(state, Duration::from_millis(100))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// A spawned child this crate still owns. Dropping it kills the process group
/// (unless `may_signal` was cleared after the leader exited) and hands the
/// child to the collector, so no error or drop path ever blocks in `wait()`.
pub(crate) struct OwnedChild {
    pub(crate) child: Option<Child>,
    pub(crate) permit: Option<Permit>,
    pub(crate) may_signal: bool,
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if self.may_signal {
                // The leader has not been reaped, so its PGID cannot be reused.
                // Descendants that move to another process group are outside this
                // supervisor's containment contract (not a sandbox).
                #[cfg(unix)]
                {
                    let pid = child.id() as libc::pid_t;
                    if pid > 1 {
                        // SAFETY: kill(2) with a negative pid only signals the
                        // group this child leads; no memory is involved.
                        unsafe {
                            libc::kill(-pid, libc::SIGKILL);
                        }
                    }
                }
                let _ = child.kill();
            }
            if let Some(permit) = self.permit.take() {
                permit.defer(child);
            }
        }
    }
}

/// Put a pipe into non-blocking mode, keeping its other status flags.
#[cfg(unix)]
pub(crate) fn set_nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: the pipe keeps owning fd across both calls; only O_NONBLOCK is added.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
