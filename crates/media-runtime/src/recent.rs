//! App-local recent-file store configuration.

use std::path::PathBuf;

pub use big_app_kit::recent_files::{RecentEntry, display_name};

const MAX_ENTRIES: usize = 50;

fn state_dir() -> PathBuf {
    big_os_kit::xdg_dirs::state_dir().map(|p|p.join(crate::config::runtime::CONFIG_DIR)).unwrap_or_default()
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
    let file_path=file_path.to_owned(); let kind=kind.to_owned();
    let timestamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    if big_os_kit::background::submit(move || {
        if store().add(&file_path,&kind,timestamp).is_err() {log::warn!("Recent-file update failed");}
    }).is_err() {log::warn!("Recent-file update was not admitted");}
}
/// Queue a clear. Use clear_recent_async when a UI needs a completion message.
pub fn clear_recent_for(kind: &str) {
    let kind=kind.to_owned();
    if big_os_kit::background::submit(move || {
        if store().clear(&kind).is_err() {log::warn!("Recent-file clear failed");}
    }).is_err() {log::warn!("Recent-file clear was not admitted");}
}
/// Acknowledge clearing recent history without blocking the UI.
pub fn clear_recent_async(kind:String, done:impl FnOnce(std::io::Result<()>)+'static) {
    crate::settings::saving::acknowledged_task(false,move ||store().clear(&kind).map_err(std::io::Error::other),done);
}
