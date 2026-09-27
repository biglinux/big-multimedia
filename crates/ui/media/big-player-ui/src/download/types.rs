//! Download manager shared types: status enum, atomic progress counters,
//! per-task UI references.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

/// Lifecycle state of a single download task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadStatus {
    /// Transfer in progress.
    Downloading,
    /// Transfer finished successfully.
    Complete,
    /// Transfer failed (see `DownloadProgress::error_msg`).
    Failed,
    /// Transfer cancelled by the user.
    Cancelled,
}

/// Atomic progress counters shared between the UI thread and the worker thread.
pub struct DownloadProgress {
    /// Bytes transferred so far.
    pub bytes_downloaded: AtomicU64,
    /// Total expected bytes (0 until known / unknown-length).
    pub total_bytes: AtomicU64,
    /// Set true when the worker has finished (success or failure).
    pub finished: AtomicBool,
    /// Set true to request cancellation; the worker polls this.
    pub cancelled: AtomicBool,
    /// Failure message, populated on error.
    pub error_msg: std::sync::Mutex<Option<String>>,
}

/// Main-thread task; UI widgets here only — worker thread reaches in via `progress`.
pub struct DownloadTask {
    /// Human-readable title shown in the row.
    pub title: String,
    /// Shared progress counters the worker thread updates.
    pub progress: Arc<DownloadProgress>,
    /// Current lifecycle state.
    pub status: Cell<DownloadStatus>,
    /// Row container widget.
    pub row_box: gtk::Box,
    /// Progress bar widget.
    pub progress_bar: gtk::ProgressBar,
    /// Status text widget.
    pub status_label: gtk::Label,
    /// Cancel button widget.
    pub cancel_button: gtk::Button,
}
