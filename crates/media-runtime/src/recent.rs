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

/// Acknowledge clearing recent history without blocking the UI.
pub fn clear_recent_async(kind: String, done: impl FnOnce(std::io::Result<()>) + 'static) {
    crate::settings::saving::acknowledged_task(
        false,
        move || store().clear(&kind).map_err(std::io::Error::other),
        done,
    );
}
