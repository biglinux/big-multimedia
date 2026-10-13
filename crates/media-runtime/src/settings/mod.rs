//! Runtime settings facade.
//!
//! Re-exports the GTK-free key/value store and typed key catalog of
//! `big_media_data::settings`, and adds the GTK side: dialog-size persistence,
//! save status and close draining.

pub use big_media_data::settings::*;

mod dialog;
pub use dialog::bind_dialog_size_persistence;

pub mod saving;
pub use saving::{acknowledged_task, attach_save_status_banner, bind_write_drain};

mod messages;
