//! A private curl transport with nonblocking pipes, a total or no-progress
//! deadline and pre-reserved reaping. No worker-per-stream, no wait() in error/drop paths.
#[cfg(unix)]
use crate::subprocess::reaper::set_nonblocking;
use crate::subprocess::reaper::{OwnedChild, Permit};
use std::io::{self, Read, Write};
use std::process::{ChildStdout, Command};
use std::time::{Duration, Instant};

pub(super) struct Transport {
    owner: Option<OwnedChild>,
    stdout: ChildStdout,
    deadline: Instant,
    /// Set for a stream: each read must get bytes within it, however long the whole transfer takes.
    stall: Option<Duration>,
    finished: bool,
    failed: bool,
}
#[cfg(unix)]
fn readiness(pipe: &impl std::os::fd::AsFd, events: i16, deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "HTTP transport timed out",
        ));
    }
    let millis = deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .clamp(1, 20) as i32;
    crate::poll::poll(&[(Some(pipe.as_fd()), events)], millis).map(drop)
}
/// Write curl's whole configuration to its nonblocking stdin before `deadline`,
/// then close it: curl reads all of it before transferring.
#[cfg(unix)]
fn send_config(
    mut stdin: std::process::ChildStdin,
    config: &[u8],
    deadline: Instant,
) -> io::Result<()> {
    set_nonblocking(&stdin)?;
    let mut sent = 0;
    while sent < config.len() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "curl configuration timed out",
            ));
        }
        match stdin.write(&config[sent..config.len().min(sent.saturating_add(65536))]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "curl configuration pipe closed",
                ));
            }
            Ok(n) => sent += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                readiness(&stdin, libc::POLLOUT, deadline)?
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
impl Transport {
    pub(super) fn new(
        mut command: Command,
        config: &[u8],
        deadline: super::Deadline,
    ) -> io::Result<Self> {
        #[cfg(not(unix))]
        {
            let _ = (&mut command, config, deadline);
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "bounded curl streaming requires Unix",
            ));
        }
        #[cfg(unix)]
        {
            let stall = match deadline {
                super::Deadline::Total(_) => None,
                super::Deadline::Stall(stall) => Some(stall),
            };
            let deadline = Instant::now()
                .checked_add(deadline.duration())
                .ok_or_else(|| io::Error::other("invalid HTTP timeout"))?;
            let permit = Permit::reserve()?;
            let mut owner = OwnedChild::spawn(&mut command, permit)?;
            let (stdin, stdout, _) = owner.take_pipes();
            let stdin = stdin.ok_or_else(|| io::Error::other("curl stdin unavailable"))?;
            send_config(stdin, config, deadline)?;
            let stdout = stdout.ok_or_else(|| io::Error::other("curl stdout unavailable"))?;
            set_nonblocking(&stdout)?;
            Ok(Self {
                owner: Some(owner),
                stdout,
                deadline,
                stall,
                finished: false,
                failed: false,
            })
        }
    }
    fn read_inner(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.failed {
            return Err(io::Error::other("HTTP transport has failed"));
        }
        if self.finished {
            return Ok(0);
        }
        if let Some(stall) = self.stall {
            self.deadline = Instant::now() + stall;
        }
        loop {
            if Instant::now() >= self.deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "HTTP transport timed out",
                ));
            }
            match self.stdout.read(buffer) {
                Ok(0) => {
                    let owner = self
                        .owner
                        .as_mut()
                        .ok_or_else(|| io::Error::other("HTTP transport closed"))?;
                    match owner.try_collect()? {
                        Some(status) => {
                            self.owner = None;
                            if status.success() {
                                self.finished = true;
                                return Ok(0);
                            }
                            return Err(io::Error::other(format!(
                                "{}{}",
                                super::CURL_EXIT_PREFIX,
                                status.code().unwrap_or(-1)
                            )));
                        }
                        None => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
                Ok(n) => return Ok(n),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    #[cfg(unix)]
                    readiness(&self.stdout, libc::POLLIN, self.deadline)?;
                    #[cfg(not(unix))]
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }
    }
}
impl Read for Transport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let result = self.read_inner(buffer);
        if result.is_err() {
            self.failed = true;
            self.owner = None;
        }
        result
    }
}
