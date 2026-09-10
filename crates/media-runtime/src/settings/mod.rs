//! Runtime settings facade.
//!
//! The GTK-free key/value store + typed key catalog live in
//! `big_media_data::settings`; re-exported here so `crate::settings::{Settings,
//! keys, …}` keep resolving. GTK dialog-size binding remains a thin adapter
//! over `big_app_kit::dialogs`.

pub use big_media_data::settings::*;

mod dialog;
pub use dialog::bind_dialog_size_persistence;
