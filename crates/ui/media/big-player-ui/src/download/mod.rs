//! Generic HTTP download manager backend: streamed transfer worker with
//! cancel/progress, filename derivation, and per-task UI state. Shared by the
//! reusable [`crate::download_panel`] and any media app.

pub mod transfer;
pub mod types;

pub use transfer::{download_file, filename_from_url};
pub use types::{DownloadProgress, DownloadStatus, DownloadTask};
