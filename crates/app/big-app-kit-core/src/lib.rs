// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]
#![forbid(unsafe_code)]

//! GTK-free app helpers behind `big-app-kit` (re-exported there unchanged).

/// Dialog size persistence.
pub mod dialogs;
/// File picker specs and display-name helpers.
pub mod files;
/// Keyboard shortcut definitions and override parsing.
pub mod keybindings;
/// Recent-files tracking, persisted between sessions.
pub mod recent_files;
/// Local HTTP remote-control endpoint (feature `remote-control`).
#[cfg(feature = "remote-control")]
pub mod remote_control;
/// Display-free zone-layout model (control catalog + per-zone ordered ids +
/// hidden set) behind the customizable button-zone editor and runtime applier.
pub mod zone_layout;
