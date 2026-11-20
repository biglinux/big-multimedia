//! Waiting for file descriptors to turn ready.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd};

/// One descriptor to watch and the `poll` events (`libc::POLLIN`, `POLLOUT`)
/// to wait for; `None` is skipped.
pub type Watch<'fd> = (Option<BorrowedFd<'fd>>, i16);

/// Waits until one of `watched` is ready or `timeout_ms` passes (`-1`: no
/// limit) and returns each one's `revents`, `0` for a skipped entry. A wait a
/// signal interrupts returns early with every entry at `0`: callers recheck
/// their state and wait again.
///
/// # Errors
/// What `poll(2)` reports other than an interruption.
pub fn poll(watched: &[Watch<'_>], timeout_ms: i32) -> io::Result<Vec<i16>> {
    let mut fds: Vec<libc::pollfd> = watched
        .iter()
        .map(|(fd, events)| libc::pollfd {
            fd: fd.as_ref().map_or(-1, AsRawFd::as_raw_fd),
            events: *events,
            revents: 0,
        })
        .collect();
    let count = libc::nfds_t::try_from(fds.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many descriptors"))?;
    // SAFETY: `fds` holds `count` initialized entries and lives through the
    // call; their descriptors are borrowed for `'fd`, so none closes meanwhile.
    // poll skips a negative descriptor.
    #[expect(clippy::disallowed_methods, reason = "the owner of poll")]
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), count, timeout_ms) };
    if ready < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        return Ok(vec![0; fds.len()]);
    }
    Ok(fds.iter().map(|fd| fd.revents).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::fd::AsFd;

    #[test]
    fn ready_pipe_reported() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        let idle = poll(
            &[(Some(reader.as_fd()), libc::POLLIN), (None, libc::POLLIN)],
            0,
        )
        .unwrap();
        assert_eq!(idle, [0, 0]);
        writer.write_all(b"x").unwrap();
        let ready = poll(
            &[(Some(reader.as_fd()), libc::POLLIN), (None, libc::POLLIN)],
            -1,
        )
        .unwrap();
        assert_ne!(ready[0] & libc::POLLIN, 0);
        assert_eq!(ready[1], 0);
    }
}
