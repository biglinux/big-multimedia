//! BigLinux Relm4 component crate.
//!
//! This crate hosts reusable [Relm4](https://relm4.org) + libadwaita components
//! shared by `big-audio-player` and `big-video-player`.
//!
//! # Module map
//!
//! - [`display`] — sleep timer dialog and texture conversion.
//! - [`feedback`] — popovers and the custom tooltip.
//! - [`input`] — sliders, dropdown rows, shortcut capture and the zone-layout editor.
//! - [`layout`] — media window chrome, dialogs, menus and toolbar panes.
//! - [`list`] — `AdwActionRow` derivatives.
//! - [`task`] — off-main-thread work with main-thread delivery.
//! - [`text`], [`time`] — markup escaping and duration formatting.
//! - [`theme`] — theme helpers shared by both players.

#![deny(missing_docs)]

pub mod display;
pub mod feedback;
pub mod i18n;
pub mod input;
pub mod layout;
pub mod list;
pub mod task;
pub mod text;
pub mod theme;
pub mod time;
