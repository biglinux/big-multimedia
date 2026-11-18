//! Bounded process-wide child collector. Reserve a permit before spawning.
//! Never creates one thread per child, never calls blocking Child::wait():
//! the collector sleeps in poll(2) on a pidfd per child until one exits.
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::Child;
use std::sync::{Arc, Mutex, PoisonError};

const MAX_CHILDREN: usize = 128;
/// Poll timeout while a child has no pidfd, because `pidfd_open` failed.
const UNWATCHED_RETRY_MS: libc::c_int = 100;

struct Deferred {
    child: Child,
    /// Readable once the child exits; `None` when `pidfd_open` failed.
    exit: Option<OwnedFd>,
}
struct State {
    reserved: usize,
    children: Vec<Deferred>,
}
struct Collector {
    state: Mutex<State>,
    /// eventfd signalled after a child is added.
    wake: OwnedFd,
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
                // SAFETY: eventfd takes no pointers and returns a new fd or -1.
                let wake = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
                if wake < 0 {
                    return Err(io::Error::last_os_error());
                }
                let collector = Arc::new(Collector {
                    state: Mutex::new(State {
                        reserved: 0,
                        children: Vec::with_capacity(MAX_CHILDREN),
                    }),
                    // SAFETY: `wake` was just created and has no other owner.
                    wake: unsafe { OwnedFd::from_raw_fd(wake) },
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
        let exit = pidfd_open(child.id());
        let mut state = self
            .collector
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // The pre-reserved slot bounds the vector; this path cannot fail due to
        // thread creation. The collector now owns the slot and the Child.
        state.children.push(Deferred { child, exit });
        self.active = false;
        drop(state);
        let one = 1_u64.to_ne_bytes();
        // SAFETY: writes 8 bytes from a live buffer to the collector's eventfd.
        // It fails only when the counter is full, which already wakes the collector.
        unsafe {
            libc::write(
                self.collector.wake.as_raw_fd(),
                one.as_ptr().cast(),
                one.len(),
            );
        }
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
        let mut watched = Vec::with_capacity(MAX_CHILDREN + 1);
        loop {
            let timeout = {
                let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                let before = state.children.len();
                state
                    .children
                    .retain_mut(|deferred| match deferred.child.try_wait() {
                        Ok(Some(_)) => false,
                        // ECHILD means an external SIGCHLD handler already collected
                        // it. Never signal a recycled PID. Other errors remain retryable.
                        Err(error) if error.raw_os_error() == Some(libc::ECHILD) => false,
                        _ => true,
                    });
                let collected = before - state.children.len();
                state.reserved -= collected;
                watched.clear();
                watched.push(readable(&self.wake));
                watched.extend(
                    state
                        .children
                        .iter()
                        .filter_map(|deferred| deferred.exit.as_ref().map(readable)),
                );
                if state
                    .children
                    .iter()
                    .any(|deferred| deferred.exit.is_none())
                {
                    UNWATCHED_RETRY_MS
                } else {
                    -1
                }
            };
            // SAFETY: `watched` is a live array of its length. Its fds stay open
            // while polling: only this thread drops children and their pidfds.
            let ready =
                unsafe { libc::poll(watched.as_mut_ptr(), watched.len() as libc::nfds_t, timeout) };
            if ready > 0 && watched[0].revents != 0 {
                let mut count = [0_u8; 8];
                // SAFETY: reads at most 8 bytes into a live buffer of that size,
                // resetting the non-blocking eventfd's counter.
                unsafe {
                    libc::read(
                        self.wake.as_raw_fd(),
                        count.as_mut_ptr().cast(),
                        count.len(),
                    );
                }
            }
        }
    }
}

fn readable(fd: &OwnedFd) -> libc::pollfd {
    libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    }
}

/// A pidfd that turns readable when the unreaped child `pid` exits; an
/// unreaped child's pid cannot be reused, so it names that child.
fn pidfd_open(pid: u32) -> Option<OwnedFd> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: pidfd_open takes no pointers and returns a new close-on-exec fd or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    let fd = libc::c_int::try_from(fd).ok().filter(|fd| *fd >= 0)?;
    // SAFETY: `fd` was just created and has no other owner.
    Some(unsafe { OwnedFd::from_raw_fd(fd) })
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

#[cfg(all(test, not(miri)))]
mod tests {
    use super::Permit;
    use std::time::{Duration, Instant};

    /// Voluntary context switches of this process's thread named `name`.
    fn thread_wakeups(name: &str) -> u64 {
        // The kernel keeps the first 15 bytes of a thread name.
        let comm = &name[..name.len().min(15)];
        for task in std::fs::read_dir("/proc/self/task").unwrap() {
            let task = task.unwrap().path();
            if std::fs::read_to_string(task.join("comm")).unwrap().trim() != comm {
                continue;
            }
            let status = std::fs::read_to_string(task.join("status")).unwrap();
            return status
                .lines()
                .find_map(|line| line.strip_prefix("voluntary_ctxt_switches:"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
        }
        panic!("no thread named {name}");
    }

    #[test]
    fn collector_sleeps_until_a_child_exits() {
        let permit = Permit::reserve().unwrap();
        let child = std::process::Command::new("sleep")
            .arg("2")
            .spawn()
            .unwrap();
        let proc_entry = format!("/proc/{}", child.id());
        let started = Instant::now();
        permit.defer(child);
        std::thread::sleep(Duration::from_millis(200));
        let before = thread_wakeups("big-child-reaper");
        std::thread::sleep(Duration::from_secs(1));
        let wakeups = thread_wakeups("big-child-reaper") - before;
        // Other tests may defer a child of their own meanwhile.
        assert!(wakeups <= 2, "{wakeups} wakeups while a child lives");
        while std::path::Path::new(&proc_entry).exists() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "exited child was not reaped"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
