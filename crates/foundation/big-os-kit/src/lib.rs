// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]

//! GTK-free OS-level contracts for BigLinux Rust apps.
//!
//! Split out of `big-app-kit` so CLI tools and non-GUI library crates
//! (camera/audio pipelines, PTY, launchers, integrations) can reuse the
//! same typed primitives without pulling in gtk4/libadwaita/relm4.
//!
//! - [`subprocess`]: argv-form subprocess specs with timeout, capture,
//!   redaction, allow-list, and a spawn-handle for long-running children.
//! - [`storage`]: atomic JSON persistence (temp + rename) and a versioned
//!   forward-migration envelope.
//! - [`keyring`]: a typed worker-only boundary to libsecret Secret Service.
//! - [`microdaemon`]: GTK-free versioned frame protocol shared by the small
//!   per-user daemons that own durable work.
//!
//! `big-app-kit` re-exports these modules unchanged, so existing
//! `big_app_kit::{subprocess, storage, keyring}` paths keep working.

/// Blocking HTTP client with timeouts, retry policy, and size limits
/// (feature `http-client`). Single home for outbound HTTP across the
/// BigLinux stack — re-exported by `big-app-kit`, consumed by data services.
/// Filesystem-safe filename sanitization.
#[cfg(feature = "http-client")]
pub mod http_client;
/// Human-readable byte/speed/duration formatting.
/// Secret-store contracts and backends.
/// Process allocator maintenance (`trim_arenas`).
pub mod mem;
/// Shared microdaemon protocol primitives.
pub mod microdaemon;
/// PulseAudio/PipeWire device discovery and control through `pactl`.
/// POSIX shell single-quoting for shell-command interpolation.
/// `ssh`/`sftp` argv builders and OpenSSH `ControlMaster` socket helpers.
/// Atomic JSON persistence helpers.
pub mod storage;
/// Safe subprocess argv specs and spawn handles.
pub mod subprocess;
/// Text-vs-binary detection and editor-mode classification for files.
/// RFC 3986 percent-encoding + URL filename helpers (GTK-free, no deps).
pub mod url;
/// Local webcam/video-node inventory helpers.
/// XDG directory resolution for BigLinux desktop apps.
pub mod xdg_dirs;

/// Bounded serial executor for application persistence.
pub mod background;

/// Bounded hashing of documents and opened files for durable receipts.
pub mod digest;
/// Worker-only Secret Service access through libsecret's secret-tool.
pub mod keyring;

/// Observable, coalesced private state writes off the GUI thread.
pub mod deferred;
