//! Runtime settings facade.
//!
//! Re-exports the GTK-free key/value store and typed key catalog of
//! `big_media_data::settings`, and adds the GTK side: save status and close
//! draining.

pub use big_media_data::settings::*;

pub mod saving;
pub use saving::{acknowledged_task, attach_save_status_banner, bind_write_drain};

mod messages;
