// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! MPRIS2 D-Bus service for desktop media applications.
//!
//! Interface XML lives next to the module so `gdbus introspect` can validate
//! the exact exported contract.

mod metadata;
mod properties;
mod service;
mod signals;
mod sync;
mod types;

/// MPRIS service wrapper.
pub use service::Mpris;
/// Private session-bus connection, shared with the tray icon.
pub(crate) use service::private_session_bus;
/// MPRIS callback and state contracts.
pub use types::{CommandCallback, MprisCommand, PlaybackStatus, PlayerState, StateCallback};

const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const LAUNCHER_ENTRY_IFACE: &str = "com.canonical.Unity.LauncherEntry";

const ROOT_XML: &str = include_str!("root.xml");
const PLAYER_XML: &str = include_str!("player.xml");
