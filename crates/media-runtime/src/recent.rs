//! App-local recent-file store configuration.

use std::path::PathBuf;

pub use big_app_kit::recent_files::{RecentEntry, display_name};

const MAX_ENTRIES: usize = 50;

fn state_dir() -> PathBuf {
    big_os_kit::xdg_dirs::state_dir()
        .unwrap_or_else(|| {
            big_os_kit::xdg_dirs::home_dir()
                .unwrap_or_default()
                .join(".local/state")
        })
        .join(crate::config::runtime::CONFIG_DIR)
}

fn store() -> big_app_kit::recent_files::RecentStore {
    big_app_kit::recent_files::RecentStore::new(state_dir(), MAX_ENTRIES)
}

/// `kind`: `"audio"` / `"video"` / `""` (legacy unkeyed).
pub fn load_recent_for(kind: &str) -> Vec<RecentEntry> {
    store().load(kind)
}

/// Dedupes by canonical path and caps at `MAX_ENTRIES`. Network URLs skipped.
pub fn add_recent_for(file_path: &str, kind: &str) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Err(err) = store().add(file_path, kind, timestamp) {
        log::warn!("Failed to save recent files: {err}");
    }
}

pub fn clear_recent_for(kind: &str) {
    if let Err(err) = store().clear(kind) {
        log::warn!("Failed to clear recent files: {err}");
    }
}
