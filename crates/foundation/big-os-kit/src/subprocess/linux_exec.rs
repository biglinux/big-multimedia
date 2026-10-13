//! Linux execution: one poll loop, no per-stream threads and no blocking join.
//! The leader remains waitable (WNOWAIT) until captured pipes reach EOF.
//! The caller must exclusively own child collection; external waitpid(-1) or
//! SIGCHLD=SIG_IGN violates the ownership contract and is reported as an error.
use super::reaper::{OwnedChild, Permit, set_nonblocking};
use super::{
    BigSubprocessError as Error, BigSubprocessOutput, BigSubprocessSpec, BigSubprocessStatus,
    DEFAULT_MAX_CAPTURE_BYTES, build_command, redact_in_place,
};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::ChildStdin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

fn exited_unreaped(pid: u32) -> io::Result<bool> {
    // SAFETY: siginfo_t is plain C data; all-zero is a valid value, and it is
    // what WNOHANG leaves behind when no child has changed state.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a live, writable siginfo_t; WNOWAIT leaves the child
    // waitable, so the PID stays ours until `try_wait` collects it.
    let rc = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: waitid succeeded, so `info` is either zeroed or a filled-in
    // SIGCHLD record; both carry a valid `si_pid`.
    Ok(unsafe { info.si_pid() } != 0)
}

fn read_turn<R: Read>(
    pipe: &mut Option<R>,
    output: &mut Vec<u8>,
    name: &'static str,
) -> Result<(), Error> {
    // At most 64 KiB per turn: a constantly readable producer must not starve
    // cancellation, deadline checks, stdin or the other output stream.
    let mut buf = [0_u8; 8192];
    for _ in 0..8 {
        let Some(reader) = pipe.as_mut() else {
            break;
        };
        match reader.read(&mut buf) {
            Ok(0) => {
                *pipe = None;
                break;
            }
            Ok(n) => {
                if n > DEFAULT_MAX_CAPTURE_BYTES.saturating_sub(output.len()) {
                    return Err(Error::OutputLimit {
                        stream: name,
                        limit: DEFAULT_MAX_CAPTURE_BYTES,
                    });
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

pub(super) fn run(
    spec: &BigSubprocessSpec,
    cancel: &AtomicBool,
) -> Result<BigSubprocessOutput, Error> {
    let deadline = Instant::now().checked_add(spec.timeout).ok_or_else(|| {
        Error::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "timeout is too large",
        ))
    })?;
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    let permit = Permit::reserve().map_err(Error::Io)?;
    let mut command = build_command(spec);
    command.process_group(0);
    let child = command.spawn().map_err(Error::Spawn)?;
    let mut owner = OwnedChild {
        child: Some(child),
        permit: Some(permit),
        may_signal: true,
    };
    let child = owner.child.as_mut().expect("owned child");
    let pid = child.id();
    let mut input = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    if let Some(pipe) = &input {
        set_nonblocking(pipe).map_err(Error::Io)?;
    }
    if let Some(pipe) = &stdout {
        set_nonblocking(pipe).map_err(Error::Io)?;
    }
    if let Some(pipe) = &stderr {
        set_nonblocking(pipe).map_err(Error::Io)?;
    }
    let bytes = spec.stdin.as_deref().unwrap_or_default();
    let mut written = 0;
    let mut out = Vec::new();
    let mut err = Vec::new();
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        read_turn(&mut stdout, &mut out, "stdout")?;
        read_turn(&mut stderr, &mut err, "stderr")?;
        write_turn(&mut input, bytes, &mut written)?;
        let exited = match exited_unreaped(pid) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => false,
            Err(error) => {
                if error.raw_os_error() == Some(libc::ECHILD) {
                    owner.may_signal = false;
                }
                return Err(Error::Io(error));
            }
        };
        if exited && input.is_none() && stdout.is_none() && stderr.is_none() {
            let code = collect(&mut owner)?;
            redact_in_place(&spec.redact_substrings, &mut out);
            redact_in_place(&spec.redact_substrings, &mut err);
            return Ok(BigSubprocessOutput {
                status: BigSubprocessStatus::Exited(code),
                stdout: out,
                stderr: err,
            });
        }
        poll_pipes(
            input.as_ref().map(AsRawFd::as_raw_fd),
            stdout.as_ref().map(AsRawFd::as_raw_fd),
            stderr.as_ref().map(AsRawFd::as_raw_fd),
            deadline,
        )?;
    }
}

/// Sleep until a pipe is ready or 20 ms pass, whichever is first, so the
/// caller rechecks cancellation and the deadline regularly.
fn poll_pipes(
    input: Option<i32>,
    stdout: Option<i32>,
    stderr: Option<i32>,
    deadline: Instant,
) -> Result<(), Error> {
    let mut fds = [
        (input, libc::POLLOUT),
        (stdout, libc::POLLIN),
        (stderr, libc::POLLIN),
    ]
    .map(|(fd, events)| libc::pollfd {
        fd: fd.unwrap_or(-1),
        events,
        revents: 0,
    });
    let remaining = deadline.saturating_duration_since(Instant::now());
    let millis = remaining.as_millis().clamp(1, 20) as i32;
    // SAFETY: fds lives through this call; poll ignores negative descriptors and
    // the others are pipes the caller still owns.
    let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) };
    if rc < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(Error::Io(error));
        }
    }
    Ok(())
}

/// Write the next stdin chunk; closes the pipe once every byte is written.
fn write_turn(
    input: &mut Option<ChildStdin>,
    bytes: &[u8],
    written: &mut usize,
) -> Result<(), Error> {
    if let Some(pipe) = input.as_mut()
        && *written < bytes.len()
    {
        let end = written.saturating_add(65536).min(bytes.len());
        match pipe.write(&bytes[*written..end]) {
            Ok(0) => {
                return Err(Error::Io(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "subprocess stdin closed",
                )));
            }
            Ok(n) => *written += n,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(Error::Io(error)),
        }
    }
    if *written == bytes.len() {
        *input = None;
    }
    Ok(())
}

/// Reap the exited leader. `try_wait` cannot block: `exited_unreaped` saw it exit.
fn collect(owner: &mut OwnedChild) -> Result<Option<i32>, Error> {
    let result = owner.child.as_mut().expect("owned child").try_wait();
    let status = match result {
        Ok(Some(status)) => status,
        Ok(None) => {
            return Err(Error::Io(io::Error::other(
                "child exit notification disappeared",
            )));
        }
        Err(error) => {
            if error.raw_os_error() == Some(libc::ECHILD) {
                owner.may_signal = false;
            }
            return Err(Error::Io(error));
        }
    };
    owner.child = None; // never signal after collection/reuse is possible
    Ok(status.code())
}
