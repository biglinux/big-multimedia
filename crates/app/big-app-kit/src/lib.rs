// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]
#![forbid(unsafe_code)]

//! GTK/libadwaita app helpers shared by `big-audio-player` and `big-video-player`.
//!
//! GTK-free pieces live in `big-app-kit-core` and `big-os-kit` and are
//! re-exported here so `big_app_kit::<module>::…` paths work for both.

/// Application and window action contracts.
pub mod actions;
/// Panic containment for Relm4 `update()` bodies and raw glib callbacks.
pub mod containment;
/// Desktop integration: action installers, file manager, power actions.
pub mod desktop;
/// Dialog size persistence (re-exported from `big-app-kit-core`).
pub mod dialogs;
/// Open/save file picker contracts.
pub mod file_dialogs;
/// Keyboard shortcut parsing, overrides and accelerator labels.
pub mod keybindings;
/// MPRIS D-Bus service for media players.
pub mod mpris;
/// System tray icon (StatusNotifierItem) with a menu.
pub mod tray;
/// File drag-and-drop helpers.
pub mod widgets;

#[cfg(feature = "remote-control")]
#[doc(inline)]
pub use big_app_kit_core::remote_control;
#[doc(inline)]
pub use big_app_kit_core::{files, recent_files, zone_layout};

#[doc(inline)]
pub use big_os_kit::{storage, subprocess};
