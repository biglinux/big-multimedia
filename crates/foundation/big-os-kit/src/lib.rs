// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]

//! GTK-free OS-level contracts for BigLinux Rust apps, so CLI tools and
//! non-GUI library crates share the same typed primitives without pulling in
//! gtk4/libadwaita/relm4.
//!
//! - `allocator`: jemalloc as the process allocator (feature `jemalloc`).
//! - [`background`]: bounded serial executor for application persistence.
//! - [`deferred`]: observable, coalesced private state writes off the GUI thread.
//! - [`digest`]: bounded hashing of documents and opened files for durable receipts.
//! - `http_client`: blocking HTTP over the system curl with timeouts, retry
//!   policy and size limits (feature `http-client`).
//! - [`keyring`]: a typed worker-only boundary to libsecret Secret Service.
//! - [`storage`]: atomic JSON persistence (temp + rename) and a versioned
//!   forward-migration envelope.
//! - [`subprocess`]: argv-form subprocess specs with timeout, capture,
//!   redaction, allow-list, and a spawn-handle for long-running children.
//! - [`url`]: RFC 3986 percent-encoding and URL file-name helpers.
//! - [`xdg_dirs`]: XDG directory resolution for BigLinux desktop apps.
//!
//! `big-app-kit` re-exports `subprocess` and `storage` unchanged.

#[cfg(feature = "jemalloc")]
mod allocator;
pub mod background;
pub mod deferred;
pub mod digest;
#[cfg(feature = "http-client")]
pub mod http_client;
pub mod keyring;
pub mod storage;
pub mod subprocess;
pub mod url;
pub mod xdg_dirs;
