// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]
#![forbid(unsafe_code)]

//! GTK-free app helpers behind `big-app-kit` (re-exported there unchanged).
//!
//! - [`dialogs`]: dialog size persistence.
//! - [`files`]: filename sanitizing and display-name helpers.
//! - [`keybindings`]: keyboard shortcut definitions and override parsing.
//! - [`recent_files`]: recent-files tracking, persisted between sessions.
//! - [`remote_control`]: local HTTP remote-control endpoint (feature `remote-control`).
//! - [`zone_layout`]: display-free zone-layout model behind the button-zone editor and applier.

pub mod dialogs;
pub mod files;
pub mod keybindings;
pub mod recent_files;
#[cfg(feature = "remote-control")]
pub mod remote_control;
pub mod zone_layout;
