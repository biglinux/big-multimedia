//! One acknowledged writer and one newest queue snapshot. Failed saves retain
//! the user's current queue for retry; normal window close can query that state.
use super::*;
use crate::db::queries_advanced::{QueueSession, load_queue_session, save_queue_session};
use big_os_kit::deferred::{Phase, Status};
use std::sync::Mutex;

#[derive(Default)]
struct State {
    desired: Option<QueueSession>,
    revision: u64,
    saved: u64,
    writing: bool,
    loaded: bool,
    failed: bool,
}

#[derive(Clone)]
pub(super) struct QueuePersistence {
    db: Option<DbHandle>,
    state: Arc<Mutex<State>>,
    changes: async_channel::Sender<()>,
}

impl QueuePersistence {
    pub(super) fn new(db: Option<DbHandle>) -> (Self, async_channel::Receiver<()>) {
        let (changes, receiver) = async_channel::bounded(1);
        (
            Self {
                db,
                state: Arc::new(Mutex::new(State::default())),
                changes,
            },
            receiver,
        )
    }

    pub(super) fn bind_failure_banner(
        &self,
        banner: &adw::Banner,
        updates: async_channel::Receiver<()>,
    ) {
        let state = Arc::downgrade(&self.state);
        let banner = banner.downgrade();
        glib::spawn_future_local(async move {
            while updates.recv().await.is_ok() {
                let (Some(state), Some(banner)) = (state.upgrade(), banner.upgrade()) else {
                    break;
                };
                banner.set_revealed(state.lock().unwrap_or_else(PoisonError::into_inner).failed);
            }
        });
    }

    pub(super) fn status(&self) -> Status {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        Status {
            phase: if state.failed {
                Phase::Failed
            } else if state.writing {
                Phase::Saving
            } else if state.revision != state.saved {
                Phase::Pending
            } else {
                Phase::Saved
            },
            revision: state.revision,
            saved_revision: state.saved,
        }
    }

    pub(super) fn unsaved(&self) -> bool {
        let status = self.status();
        status.revision != status.saved_revision
    }

    pub(super) fn loaded(&mut self, database: Option<DbHandle>) {
        let success = database.is_some();
        if let Some(database) = database {
            self.db = Some(database);
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.loaded = success;
        state.failed = !success;
        drop(state);
        let _ = self.changes.try_send(());
        if success {
            self.retry();
        }
    }

    pub(super) fn replace(&self, session: QueueSession) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.desired = Some(session);
        state.revision += 1;
        drop(state);
        self.retry();
    }

    pub(super) fn retry(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.writing || !state.loaded || state.revision == state.saved {
            return;
        }
        let Some(db) = self.db.clone() else {
            state.failed = true;
            let _ = self.changes.try_send(());
            return;
        };
        state.writing = true;
        state.failed = false;
        drop(state);
        let _ = self.changes.try_send(());
        let owner = self.clone();
        let admitted = big_os_kit::background::submit(move || {
            loop {
                let (session, revision) = {
                    let state = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
                    (
                        state.desired.clone().expect("pending queue snapshot"),
                        state.revision,
                    )
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
                    save_queue_session(&conn, &session)
                }));
                let mut state = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
                if matches!(result, Ok(Ok(()))) {
                    state.saved = revision;
                    if state.revision != revision {
                        continue;
                    }
                    state.desired = None;
                } else {
                    state.failed = true;
                }
                state.writing = false;
                let _ = owner.changes.try_send(());
                break;
            }
        });
        if admitted.is_err() {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.writing = false;
            state.failed = true;
            let _ = self.changes.try_send(());
        }
    }

    pub(super) fn load(&self, sender: &ComponentSender<AudioShell>) {
        if self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .loaded
        {
            self.retry();
            return;
        }
        let db = self.db.clone();
        let input = sender.input_sender().clone();
        spawn_blocking_result(
            move || {
                let db = match db {
                    Some(db) => db,
                    None => db::open_database().map_err(|_| ())?,
                };
                let session = {
                    let conn = db.lock().map_err(|_| ())?;
                    load_queue_session(&conn).map_err(|_| ())?
                };
                Ok((db, session))
            },
            move |result| {
                let _ = input.send(AudioShellInput::QueueRestored(result));
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::queries_advanced::SavedQueue;

    fn wait_for_result(owner: &QueuePersistence) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while matches!(owner.status().phase, Phase::Saving | Phase::Pending) {
            assert!(
                std::time::Instant::now() < deadline,
                "queue save never completed"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn session(path: &str) -> QueueSession {
        QueueSession {
            music: SavedQueue {
                tracks: vec![Track::new_external_path(path)],
                current: Some(0),
            },
            podcasts: SavedQueue::default(),
        }
    }

    #[test]
    fn newest_edit_wins_and_failed_save_can_be_retried() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::schema::run_migrations(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let (mut owner, _updates) = QueuePersistence::new(Some(db.clone()));
        owner.loaded(Some(db.clone()));
        let lock = db.lock().unwrap();
        lock.execute_batch(
            "CREATE TEMP TABLE write_count (value INTEGER);
            CREATE TEMP TRIGGER count_queue_writes AFTER INSERT ON playback_queue_items
            WHEN NEW.queue_name='music' BEGIN INSERT INTO write_count VALUES(1); END;",
        )
        .unwrap();
        for index in 0..1000 {
            owner.replace(session(&format!("/{index}.flac")));
        }
        assert!(owner.unsaved());
        assert!(big_os_kit::background::pending() > 0);
        drop(lock);
        wait_for_result(&owner);
        assert!(!owner.unsaved());
        let writes: i64 = db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM write_count", [], |row| row.get(0))
            .unwrap();
        assert!(
            writes <= 2,
            "1000 edits should retain only the active and newest save, got {writes}"
        );
        assert_eq!(
            load_queue_session(&db.lock().unwrap())
                .unwrap()
                .music
                .tracks[0]
                .file_path,
            "/999.flac"
        );
        db.lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_queue BEFORE INSERT ON playback_queue_items
            BEGIN SELECT RAISE(ABORT, 'write failed'); END;",
            )
            .unwrap();
        owner.replace(session("/new.flac"));
        wait_for_result(&owner);
        assert_eq!(owner.status().phase, Phase::Failed);
        assert!(owner.unsaved());
        assert_eq!(
            load_queue_session(&db.lock().unwrap())
                .unwrap()
                .music
                .tracks[0]
                .file_path,
            "/999.flac"
        );
        db.lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_queue")
            .unwrap();
        owner.retry();
        wait_for_result(&owner);
        assert!(!owner.unsaved());
        assert_eq!(
            load_queue_session(&db.lock().unwrap())
                .unwrap()
                .music
                .tracks[0]
                .file_path,
            "/new.flac"
        );
    }
}
