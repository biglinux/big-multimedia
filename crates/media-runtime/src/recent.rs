//! App-local recent-file store configuration.

pub use big_app_kit::recent_files::{RecentEntry, display_name};

const MAX_ENTRIES: usize = 50;

/// `None` without a state directory: no recent files are kept.
fn store() -> Option<big_app_kit::recent_files::RecentStore> {
    let dir = big_os_kit::xdg_dirs::state_dir()?.join(crate::config::runtime::CONFIG_DIR);
    Some(big_app_kit::recent_files::RecentStore::new(
        dir,
        MAX_ENTRIES,
    ))
}

/// `kind`: `"audio"` or `"video"`.
pub fn load_recent_for(kind: &str) -> Vec<RecentEntry> {
    store().map_or_else(Vec::new, |store| store.load(kind))
}

/// Record `path` as just opened, on a worker thread (file lock + atomic write).
pub fn add_recent_async(kind: &'static str, path: String) {
    let Some(store) = store() else {
        return;
    };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    crate::settings::saving::acknowledged_task(
        false,
        move || {
            store
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
