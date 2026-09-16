//! A private curl transport with nonblocking pipes, an operation deadline and
//! pre-reserved reaping. No worker-per-stream, no wait() in error/drop paths.
use std::io::{self, Read, Write};
use std::process::{Child, ChildStdout, Command};
use std::time::{Duration, Instant};
use crate::subprocess::reaper::Permit;

struct Owner { child: Option<Child>, permit: Option<Permit>, may_signal: bool }
impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if self.may_signal {
                #[cfg(unix)] { let pid = child.id() as libc::pid_t; if pid > 1 { unsafe { libc::kill(-pid, libc::SIGKILL); } } }
                let _ = child.kill();
            }
            if let Some(permit) = self.permit.take() { permit.defer(child); }
        }
    }
}
pub(super) struct Transport { owner: Option<Owner>, stdout: ChildStdout, deadline: Instant, finished: bool, failed: bool }
#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: pipe retains ownership during both calls.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 { return Err(io::Error::last_os_error()); }
    Ok(())
}
#[cfg(unix)]
fn readiness(pipe: &impl std::os::fd::AsRawFd, events: i16, deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "HTTP transport timed out")); }
    let mut fd = libc::pollfd { fd: pipe.as_raw_fd(), events, revents: 0 };
    let millis = deadline.saturating_duration_since(Instant::now()).as_millis().clamp(1, 20) as i32;
    // SAFETY: stack pollfd and its owned pipe remain valid until poll returns.
    if unsafe { libc::poll(&mut fd, 1, millis) } < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted { return Err(error); }
    }
    Ok(())
}
impl Transport {
    pub(super) fn new(mut command: Command, config: &[u8], timeout: Duration) -> io::Result<Self> {
        #[cfg(not(unix))] { let _ = (&mut command, config, timeout); return Err(io::Error::new(io::ErrorKind::Unsupported, "bounded curl streaming requires Unix")); }
        #[cfg(unix)] {
            use std::os::unix::process::CommandExt;
            let deadline = Instant::now().checked_add(timeout).ok_or_else(|| io::Error::other("invalid HTTP timeout"))?;
            let permit = Permit::reserve()?;
            command.process_group(0);
            let child = command.spawn()?;
            let mut owner = Owner { child: Some(child), permit: Some(permit), may_signal: true };
            crate::subprocess::notify_spawn_observer(owner.child.as_ref().expect("owned child").id(), "curl");
            let mut stdin = owner.child.as_mut().expect("owned child").stdin.take().ok_or_else(|| io::Error::other("curl stdin unavailable"))?;
            nonblocking(&stdin)?;
            let mut sent = 0;
            while sent < config.len() {
                if Instant::now() >= deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "curl configuration timed out")); }
                match stdin.write(&config[sent..config.len().min(sent.saturating_add(65536))]) {
                    Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "curl configuration pipe closed")),
                    Ok(n) => sent += n,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => readiness(&stdin, libc::POLLOUT, deadline)?,
                    Err(error) => return Err(error),
                }
            }
            drop(stdin); // curl reads its whole configuration before transferring
            let stdout = owner.child.as_mut().expect("owned child").stdout.take().ok_or_else(|| io::Error::other("curl stdout unavailable"))?;
            nonblocking(&stdout)?;
            Ok(Self { owner: Some(owner), stdout, deadline, finished: false, failed: false })
        }
    }
    fn read_inner(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.failed { return Err(io::Error::other("HTTP transport has failed")); }
        if self.finished { return Ok(0); }
        loop {
            if Instant::now() >= self.deadline { return Err(io::Error::new(io::ErrorKind::TimedOut, "HTTP transport timed out")); }
            match self.stdout.read(buffer) {
                Ok(0) => {
                    let owner = self.owner.as_mut().ok_or_else(|| io::Error::other("HTTP transport closed"))?;
                    let result = owner.child.as_mut().expect("owned child").try_wait();
                    match result {
                        Ok(Some(status)) => {
                            owner.child = None; // collected: never signal this PID/group again
                            self.owner = None;
                            if status.success() { self.finished = true; return Ok(0); }
                            return Err(io::Error::other(format!("curl exited with code {}", status.code().unwrap_or(-1))));
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                        Err(error) => {
                            #[cfg(unix)] if error.raw_os_error() == Some(libc::ECHILD) { owner.may_signal = false; }
                            return Err(error);
                        }
                    }
                }
                Ok(n) => return Ok(n),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    #[cfg(unix)] readiness(&self.stdout, libc::POLLIN, self.deadline)?;
                    #[cfg(not(unix))] return Err(error);
                }
                Err(error) => return Err(error),
            }
        }
    }
}
impl Read for Transport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() { return Ok(0); }
        let result = self.read_inner(buffer);
        if result.is_err() { self.failed = true; self.owner = None; }
        result
    }
}
