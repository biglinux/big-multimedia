//! Smartphone web-remote dialog and QR pairing surface.

mod dialog;
mod qr;

pub use big_app_kit::remote_control::{RemoteHandlers, StatusSnapshot};
pub use dialog::present;
