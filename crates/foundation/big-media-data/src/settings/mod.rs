//! Persistent app settings: GTK-free key/value store on disk plus typed key
//! catalog. The GTK dialog-size persistence helper lives app/runtime-side
//! (it binds an `adw::Window`); see `big_media_runtime::settings`.

use std::collections::HashMap;
use std::path::PathBuf;

pub mod keys;

mod defaults;
mod schema;
mod store;
#[cfg(test)]
mod tests;

pub use store::Settings;

struct Inner {
    settings_values: HashMap<String, String>,
    path: PathBuf,
    dirty: bool,
    disk_bytes: Option<Vec<u8>>,
    load_error: Option<String>,
    durability_pending: bool,
}
