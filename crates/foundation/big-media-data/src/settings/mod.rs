//! Persistent app settings: GTK-free key/value store on disk plus typed key
//! catalog.

use std::collections::HashMap;
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
    disk_bytes: Option<Vec<u8>>,
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
