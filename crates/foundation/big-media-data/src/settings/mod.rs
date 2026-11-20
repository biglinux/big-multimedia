//! Persistent app settings: GTK-free key/value store on disk plus typed key
//! catalog.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub mod keys;

mod schema;
mod store;
#[cfg(test)]
mod tests;

pub use store::{SavePhase, SaveStatus, Settings};

struct Inner {
    settings_values: HashMap<String, String>,
    path: PathBuf,
    dirty: bool,
    /// Keys this process set since they were last saved. A save writes these
    /// over the file as it is then, so another process's keys survive.
    changed: HashSet<String>,
    load_error: Option<String>,
    revision: u64,
    persisted_revision: u64,
    save_queued: bool,
    phase: store::SavePhase,
    save_error: Option<String>,
    // Never acquire this lock on the GUI thread. It serializes disk transactions
    // without holding the in-memory model across filesystem operations.
    disk_lock: std::sync::Arc<std::sync::Mutex<()>>,
}
