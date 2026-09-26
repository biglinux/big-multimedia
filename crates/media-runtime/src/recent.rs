//! App-local recent-file store configuration.

use std::path::PathBuf;

pub use big_app_kit::recent_files::{RecentEntry, display_name};

const MAX_ENTRIES: usize = 50;

fn state_dir() -> PathBuf {
    big_os_kit::xdg_dirs::state_dir()
        .map(|p| p.join(crate::config::runtime::CONFIG_DIR))
        .unwrap_or_default()
}

fn store() -> big_app_kit::recent_files::RecentStore {
    big_app_kit::recent_files::RecentStore::new(state_dir(), MAX_ENTRIES)
}

/// `kind`: `"audio"` / `"video"` / `""` (legacy unkeyed).
pub fn load_recent_for(kind: &str) -> Vec<RecentEntry> {
    store().load(kind)
}

/// Record `path` as just opened, on a worker thread (file lock + atomic write).
pub fn add_recent_async(kind: &'static str, path: String) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    crate::settings::saving::acknowledged_task(
        false,
        move || {
            store()
                .add(&path, kind, timestamp)
                .map_err(std::io::Error::other)
        },
        |result| {
            if let Err(err) = result {
                log::warn!("recent: could not record opened file: {err}");
            }
        },
    );
}
