//! Observable, coalesced background saves for small private state documents.
//! Callers provide the exact bytes they loaded; comparison and publication occur
//! under one cooperating-writer lock. This is not a user-document overwrite API.
//! Saves are atomic but not fsynced: losing the last seconds of player state to
//! a power cut is acceptable, a sync per edit is not.
use crate::storage;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

/// Per-document allocation/read limit. Larger user documents need their own policy.
pub const MAX_BYTES: usize = 1024 * 1024;
/// Acknowledged phase, never inferred from merely submitting a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The latest local generation is written (the kernel flushes it later).
    Saved,
    /// An admitted or superseding save is pending.
    Pending,
    /// A transaction is running outside the UI thread.
    Saving,
    /// Save failed or the loaded document was invalid; edits remain in memory.
    Failed,
}
/// Snapshot for presentation. It contains no paths, document bytes or credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Current state of persistence.
    pub phase: Phase,
    /// Latest locally edited generation.
    pub revision: u64,
    /// Last generation written to the file.
    pub saved_revision: u64,
}
struct State {
    expected: Option<Vec<u8>>,
    desired: Option<Vec<u8>>,
    revision: u64,
    saved_revision: u64,
    queued: bool,
    blocked: bool,
    invalid_update: bool,
    phase: Phase,
}
/// Shared persistence owner. Workers retain it until completion, independent of widgets.
#[derive(Clone)]
pub struct Document {
    path: Arc<PathBuf>,
    state: Arc<Mutex<State>>,
}
impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferredDocument")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}
static OBSERVED: std::sync::OnceLock<Mutex<Vec<std::sync::Weak<Mutex<State>>>>> =
    std::sync::OnceLock::new();
/// Whether any live independent document has an unsaved/rejected update. Weak
/// owners do not retain private bytes after the document and its worker are gone.
pub fn any_unsaved() -> bool {
    let refs = OBSERVED
        .get()
        .map(|items| items.lock().unwrap_or_else(PoisonError::into_inner).clone())
        .unwrap_or_default();
    refs.iter()
        .filter_map(std::sync::Weak::upgrade)
        .any(|owner| {
            let s = owner.lock().unwrap_or_else(PoisonError::into_inner);
            s.invalid_update || s.revision != s.saved_revision
        })
}
impl Document {
    /// Construct with a validated, exact disk snapshot; None means confirmed missing.
    /// Does not access the filesystem or create directories.
    pub fn reviewed(path: PathBuf, expected: Option<Vec<u8>>) -> Self {
        let blocked = !path.is_absolute() || expected.as_ref().is_some_and(|v| v.len() > MAX_BYTES);
        let document = Self {
            path: Arc::new(path),
            state: Arc::new(Mutex::new(State {
                desired: expected.clone(),
                expected,
                revision: 0,
                saved_revision: 0,
                queued: false,
                blocked,
                invalid_update: false,
                phase: if blocked { Phase::Failed } else { Phase::Saved },
            })),
        };
        let mut items = OBSERVED
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        items.retain(|item| item.strong_count() > 0);
        items.push(Arc::downgrade(&document.state));
        document
    }
    /// Invalid/unreadable source: edits are session-only until the application reloads
    /// and explicitly resolves the conflict. Never silently overwrite malformed state.
    pub fn blocked(path: PathBuf) -> Self {
        let value = Self::reviewed(path, None);
        {
            let mut s = value.state.lock().unwrap_or_else(PoisonError::into_inner);
            s.blocked = true;
            s.phase = Phase::Failed;
        }
        value
    }
    /// Current acknowledged state, with no blocking disk work.
    pub fn status(&self) -> Status {
        let s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        Status {
            phase: s.phase,
            revision: s.revision,
            saved_revision: s.saved_revision,
        }
    }
    /// Record validated bytes and admit an asynchronous save. Newer edits replace
    /// only the pending bytes, never the currently running transaction's snapshot.
    pub fn replace(&self, bytes: Vec<u8>) -> io::Result<()> {
        if bytes.len() > MAX_BYTES {
            self.reject_update();
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "private state exceeds its limit",
            ));
        }
        {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            s.invalid_update = false;
            if s.desired.as_ref() != Some(&bytes) {
                s.revision = s
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("state generation exhausted"))?;
                s.desired = Some(bytes);
                // A running save may already have published `Saved` for the
                // previous generation; its worker requeues this one.
                s.phase = Phase::Pending;
            }
        }
        self.retry()
    }
    /// Record a rejected serialization/validation attempt without claiming that
    /// the rejected in-memory content was saved. A new valid replace clears it.
    pub fn reject_update(&self) {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.invalid_update = true;
        s.phase = Phase::Failed;
    }

    /// Retry a failed save. Never rebases onto an external writer's content:
    /// a conflict needs an explicit reload.
    pub fn retry(&self) -> io::Result<()> {
        {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if s.invalid_update {
                s.phase = Phase::Failed;
                return Err(io::Error::other(
                    "invalid local state; correct it before saving",
                ));
            }
            if s.blocked {
                s.phase = Phase::Failed;
                return Err(io::Error::other(
                    "saved state is invalid or unavailable; reload before saving",
                ));
            }
            if s.queued {
                return Ok(());
            }
            if s.revision == s.saved_revision {
                s.phase = Phase::Saved;
                return Ok(());
            }
            s.queued = true;
            s.phase = Phase::Pending;
        }
        let owner = self.clone();
        let admitted = crate::background::submit(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.transaction()));
            let success = matches!(result, Ok(Ok(())));
            let again = {
                let mut s = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
                s.queued = false;
                if !success {
                    s.phase = Phase::Failed;
                }
                if s.invalid_update {
                    s.phase = Phase::Failed;
                }
                success && !s.invalid_update && s.revision != s.saved_revision
            };
            if again {
                let _ = owner.retry();
            }
        });
        if admitted.is_err() {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            s.queued = false;
            s.phase = Phase::Failed;
        }
        admitted
    }
    fn transaction(&self) -> io::Result<()> {
        let (expected, desired, revision) = {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            s.phase = Phase::Saving;
            (s.expected.clone(), s.desired.clone(), s.revision)
        };
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("missing private directory"))?;
        std::fs::create_dir_all(parent)?;
        let _lock = storage::WriteLock::try_acquire(&self.path)?;
        if storage::read_regular_limited(&self.path, MAX_BYTES)? != expected {
            return Err(io::Error::other(
                "state changed in another instance; reload before saving",
            ));
        }
        if desired != expected {
            let bytes = desired.ok_or_else(|| io::Error::other("no state submitted"))?;
            storage::atomic_write_private(&self.path, &bytes)?;
            // Merge only publication state, not desired values that may have
            // changed during I/O. This short lock is never held around a syscall.
            self.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .expected = Some(bytes);
        }
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.saved_revision = revision;
        s.phase = if s.revision == revision {
            Phase::Saved
        } else {
            Phase::Pending
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn path(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "deferred-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        p.join(name)
    }
    fn drain(doc: &Document) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while matches!(doc.status().phase, Phase::Pending | Phase::Saving)
            && std::time::Instant::now() < until
        {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!matches!(
            doc.status().phase,
            Phase::Pending | Phase::Saving
        ));
    }
    #[test]
    fn newer_generations_survive_an_inflight_save() {
        let p = path("prefs.json");
        let doc = Document::reviewed(p.clone(), None);
        for n in 0..100 {
            doc.replace(n.to_string().into_bytes()).unwrap();
            assert_ne!(doc.status().phase, Phase::Saved);
        }
        drain(&doc);
        assert_eq!(doc.status().phase, Phase::Saved);
        assert_eq!(std::fs::read(&p).unwrap(), b"99");
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn conflict_never_rebases_implicitly() {
        let p = path("prefs.json");
        std::fs::write(&p, b"before").unwrap();
        let doc = Document::reviewed(p.clone(), Some(b"before".to_vec()));
        std::fs::write(&p, b"external").unwrap();
        doc.replace(b"ours".to_vec()).unwrap();
        drain(&doc);
        assert_eq!(doc.status().phase, Phase::Failed);
        assert_eq!(std::fs::read(&p).unwrap(), b"external");
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
    #[test]
    fn invalid_source_and_relative_path_never_publish() {
        let doc = Document::blocked(PathBuf::from("relative"));
        assert!(doc.replace(b"new".to_vec()).is_err());
        assert_eq!(doc.status().phase, Phase::Failed);
    }
}
