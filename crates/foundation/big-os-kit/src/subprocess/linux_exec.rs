//! Linux execution: one poll loop, no per-stream threads and no blocking join.
//! The leader remains waitable (WNOWAIT) until captured pipes reach EOF.
//! The caller must exclusively own child collection; external waitpid(-1) or
//! SIGCHLD=SIG_IGN violates the ownership contract and is reported as an error.
use super::{BigSubprocessError as Error, BigSubprocessSpec, BigSubprocessOutput,
    BigSubprocessStatus, DEFAULT_MAX_CAPTURE_BYTES, build_command, notify_spawn_observer,
    redact_in_place};
use super::reaper::Permit;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

struct OwnedChild { child: Option<Child>, permit: Option<Permit>, may_signal: bool }
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if self.may_signal {
                // The leader has not been reaped, so PGID cannot be reused.
                // Descendants that deliberately escape the group are outside
                // this supervisor's containment contract (not a sandbox).
                let pid = child.id() as libc::pid_t;
                if pid > 1 {
                    unsafe { libc::kill(-pid, libc::SIGKILL); }
                }
                let _ = child.kill();
            }
            if let Some(permit) = self.permit.take() { permit.defer(child); }
        }
    }
}

fn nonblocking(stream: &impl AsRawFd) -> io::Result<()> {
    let fd = stream.as_raw_fd();
    // SAFETY: stream owns fd throughout both calls; flags preserve other bits.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 { return Err(io::Error::last_os_error()); }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn exited_unreaped(pid: u32) -> io::Result<bool> {
    // SAFETY: siginfo_t is an output C POD; initializing also covers no-event.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info,
        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) };
    if rc < 0 { return Err(io::Error::last_os_error()); }
    Ok(unsafe { info.si_pid() } != 0)
}

fn read_turn<R: Read>(pipe: &mut Option<R>, output: &mut Vec<u8>, name: &'static str) -> Result<(), Error> {
    // At most 64 KiB per turn: a constantly readable producer must not starve
    // cancellation, deadline checks, stdin or the other output stream.
    let mut buf = [0_u8; 8192];
    for _ in 0..8 {
        let Some(reader) = pipe.as_mut() else { break; };
        match reader.read(&mut buf) {
            Ok(0) => { *pipe = None; break; }
            Ok(n) => {
                if n > DEFAULT_MAX_CAPTURE_BYTES.saturating_sub(output.len()) {
                    return Err(Error::OutputLimit { stream: name, limit: DEFAULT_MAX_CAPTURE_BYTES });
                }
                output.extend_from_slice(&buf[..n]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(Error::Io(error)),
        }
    }
    Ok(())
}

pub(super) fn run(spec: &BigSubprocessSpec, cancel: &AtomicBool) -> Result<BigSubprocessOutput, Error> {
    let deadline = Instant::now().checked_add(spec.timeout)
        .ok_or_else(|| Error::Io(io::Error::new(io::ErrorKind::InvalidInput, "timeout is too large")))?;
    if cancel.load(Ordering::Acquire) { return Err(Error::Cancelled); }
    if Instant::now() >= deadline { return Err(Error::Timeout); }
    let permit = Permit::reserve().map_err(Error::Io)?;
    let mut command = build_command(spec);
    command.process_group(0);
    let child = command.spawn().map_err(Error::Spawn)?;
    let mut owner = OwnedChild { child: Some(child), permit: Some(permit), may_signal: true };
    let child = owner.child.as_mut().expect("owned child");
    let pid = child.id();
    // Guard is already installed if an observer unexpectedly panics.
    notify_spawn_observer(pid, &spec.program);
    let mut input = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    if let Some(pipe) = &input { nonblocking(pipe).map_err(Error::Io)?; }
    if let Some(pipe) = &stdout { nonblocking(pipe).map_err(Error::Io)?; }
    if let Some(pipe) = &stderr { nonblocking(pipe).map_err(Error::Io)?; }
    let bytes = spec.stdin.as_deref().unwrap_or_default();
    let mut written = 0;
    let mut out = Vec::new();
    let mut err = Vec::new();
    loop {
        if cancel.load(Ordering::Acquire) { return Err(Error::Cancelled); }
        if Instant::now() >= deadline { return Err(Error::Timeout); }
        read_turn(&mut stdout, &mut out, "stdout")?;
        read_turn(&mut stderr, &mut err, "stderr")?;
        if written == bytes.len() { input = None; }
        if let Some(pipe) = input.as_mut() {
            let end = written.saturating_add(65536).min(bytes.len());
            match pipe.write(&bytes[written..end]) {
                Ok(0) => return Err(Error::Io(io::Error::new(io::ErrorKind::WriteZero, "subprocess stdin closed"))),
                Ok(n) => written += n,
                Err(error) if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) => {},
                Err(error) => return Err(Error::Io(error)),
            }
            if written == bytes.len() { input = None; }
        }
        let exited = match exited_unreaped(pid) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => false,
            Err(error) => {
                if error.raw_os_error() == Some(libc::ECHILD) { owner.may_signal = false; }
                return Err(Error::Io(error));
            }
        };
        if exited && input.is_none() && stdout.is_none() && stderr.is_none() {
            // Reap only once the operation is complete. try_wait cannot block.
            let status = owner.child.as_mut().expect("owned child").try_wait().map_err(Error::Io)?
                .ok_or_else(|| Error::Io(io::Error::other("child exit notification disappeared")))?;
            owner.child = None; // never signal after collection/reuse is possible
            redact_in_place(&spec.redact_substrings, &mut out);
            redact_in_place(&spec.redact_substrings, &mut err);
            return Ok(BigSubprocessOutput { status: BigSubprocessStatus::Exited(status.code()), stdout: out, stderr: err });
        }
        let mut fds = [
            libc::pollfd { fd: input.as_ref().map_or(-1, AsRawFd::as_raw_fd), events: libc::POLLOUT, revents: 0 },
            libc::pollfd { fd: stdout.as_ref().map_or(-1, AsRawFd::as_raw_fd), events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: stderr.as_ref().map_or(-1, AsRawFd::as_raw_fd), events: libc::POLLIN, revents: 0 },
        ];
        let remaining = deadline.saturating_duration_since(Instant::now());
        let millis = remaining.as_millis().clamp(1, 20) as i32;
        // SAFETY: fds lives through this call; only descriptors owned above.
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) };
        if rc < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted { return Err(Error::Io(error)); }
        }
    }
}
