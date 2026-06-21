//! Interactive trim mode: enter a non-modal editing state over the playing
//! video, mark one or more cut segments on a timeline, then export them either
//! fast (stream-copy, keep format) or precise (re-encode), with live progress +
//! cancel. Reuses the shared conversion engine (`big_converter_ui::export`) and
//! the shared progress component (`big_media_components::converter::progress`).
//!
//! - [`state`] — pure segment/mark logic (unit-tested).
//! - [`bar`] — the `VideoTrimBar` Relm4 component (timeline + controls).
//! - [`run`] — the shell-side glue (`impl VideoShell`): enter/exit, marking
//!   from the live mpv position, and driving the export + progress overlay.

pub(super) mod bar;
mod run;
mod state;

pub(super) use bar::{VideoTrimBar, VideoTrimBarOutput};
pub(super) use run::TrimExportOutcome;
