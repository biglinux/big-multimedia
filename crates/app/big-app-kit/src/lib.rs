// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

#![deny(missing_docs)]
#![forbid(unsafe_code)]

//! GTK/libadwaita app helpers shared by `big-audio-player` and `big-video-player`.
//!
//! GTK-free pieces live in `big-app-kit-core` and `big-os-kit` and are
//! re-exported here so `big_app_kit::<module>::…` paths work for both.
//!
//! - [`actions`]: application and window action contracts.
//! - [`containment`]: panic containment for Relm4 `update()` bodies and raw glib callbacks.
//! - [`desktop`]: desktop integration: action installers, file manager, power actions.
//! - [`file_dialogs`]: open/save file picker contracts.
//! - [`keybindings`]: keyboard shortcut parsing, overrides and accelerator labels.
//! - [`mpris`]: MPRIS D-Bus service for media players.
//! - [`tray`]: system tray icon (StatusNotifierItem) with a menu.
//! - [`widgets`]: file drag-and-drop helpers.

pub mod actions;
pub mod containment;
pub mod desktop;
pub mod file_dialogs;
pub mod keybindings;
pub mod mpris;
pub mod tray;
pub mod widgets;

#[cfg(feature = "remote-control")]
#[doc(inline)]
pub use big_app_kit_core::remote_control;
#[doc(inline)]
pub use big_app_kit_core::{files, recent_files, zone_layout};

#[doc(inline)]
pub use big_os_kit::{storage, subprocess};
