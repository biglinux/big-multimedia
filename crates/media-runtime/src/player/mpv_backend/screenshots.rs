//! Screenshot resources belong to the backend, never to a notice timer.
//! Timeout withdraws publication and requests cancellation. Resources stay owned
//! until COMMAND_REPLY or until mpv_terminate_destroy has joined native writers.
use big_os_kit::storage::{CommitOutcome, StagedFile};
use std::collections::HashMap;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const LIMIT: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub enum ScreenshotResult {
    Saved {
        path: PathBuf,
        durability_confirmed: bool,
    },
    Failed,
    Cancelled,
}
struct Request {
    stage: StagedFile,
    target: PathBuf,
    alive: Arc<AtomicBool>,
    deadline: Instant,
    withdrawn: bool,
}
struct State {
    next: u64,
    requests: HashMap<u64, Request>,
}
pub(super) struct Screenshots(Mutex<State>);
impl Screenshots {
    pub(super) fn new() -> Self {
        Self(Mutex::new(State {
            next: 1,
            requests: HashMap::with_capacity(LIMIT),
        }))
    }
    pub(super) fn register(
        &self,
        stage: StagedFile,
        target: PathBuf,
        alive: Arc<AtomicBool>,
    ) -> io::Result<u64> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !alive.load(Ordering::Acquire) {
            return Err(io::Error::other("screenshot owner closed"));
        }
        if state.requests.len() >= LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "screenshot queue is full",
            ));
        }
        let id = loop {
            let id = state.next;
            state.next = state.next.wrapping_add(1).max(1);
            if id != 0 && !state.requests.contains_key(&id) {
                break id;
            }
        };
        state.requests.insert(
            id,
            Request {
                stage,
                target,
                alive,
                deadline: Instant::now() + TIMEOUT,
                withdrawn: false,
            },
        );
        Ok(id)
    }
    // Called only when mpv rejected enqueueing, so no native writer exists.
    pub(super) fn rejected(&self, id: u64) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .requests
            .remove(&id);
    }
    pub(super) fn withdraw_expired(&self) -> Vec<u64> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let mut ids = Vec::new();
        for (&id, request) in &mut state.requests {
            if !request.withdrawn
                && (now >= request.deadline || !request.alive.load(Ordering::Acquire))
            {
                request.withdrawn = true;
                ids.push(id);
            }
        }
        ids
    }
    /// COMMAND_REPLY is the terminal receipt. Remove ownership only here, after
    /// native completion; a prior timeout never deletes a live writer's output.
    pub(super) fn complete(&self, id: u64, error: i32) -> Option<ScreenshotResult> {
        let request = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .requests
            .remove(&id)?;
        if request.withdrawn
            || Instant::now() >= request.deadline
            || !request.alive.load(Ordering::Acquire)
        {
            return Some(ScreenshotResult::Cancelled);
        }
        if error < 0 {
            return Some(ScreenshotResult::Failed);
        }
        match validate_png(request.stage.path())
            .and_then(|()| request.stage.publish(&request.target))
        {
            Ok(outcome) => Some(ScreenshotResult::Saved {
                path: request.target,
                durability_confirmed: matches!(outcome, CommitOutcome::Durable),
            }),
            Err(error) => {
                log::warn!("Screenshot output could not be published: {}", error.kind());
                Some(ScreenshotResult::Failed)
            }
        }
    }
    /// Call only after mpv_terminate_destroy, when no writer can recreate these
    /// staging files.
    pub(super) fn clear_after_termination(&self) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .requests
            .clear();
    }
}
fn validate_png(path: &std::path::Path) -> io::Result<()> {
    use std::fs::OpenOptions;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("screenshot is not regular"));
    }
    let mut signature = [0_u8; 8];
    file.read_exact(&mut signature)?;
    if &signature != b"\x89PNG\r\n\x1a\n" {
        return Err(io::Error::other("unexpected screenshot encoding"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn withdrawn_request_keeps_staging_until_terminal_reply() {
        let dir = std::env::temp_dir().join(format!("screenshot-receipt-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let target = dir.join("shot.png");
        let stage = StagedFile::new(&target).unwrap();
        let private = stage.path().to_owned();
        let alive = Arc::new(AtomicBool::new(true));
        let registry = Screenshots::new();
        let id = registry
            .register(stage, target.clone(), alive.clone())
            .unwrap();
        alive.store(false, Ordering::Release);
        assert_eq!(registry.withdraw_expired(), vec![id]);
        assert!(private.exists()); // not cleaned at cancellation request
        std::fs::write(&private, b"\x89PNG\r\n\x1a\nlate bytes").unwrap();
        assert!(matches!(
            registry.complete(id, 0),
            Some(ScreenshotResult::Cancelled)
        ));
        assert!(!private.exists());
        assert!(!target.exists());
        std::fs::remove_dir(dir).unwrap();
    }
}
