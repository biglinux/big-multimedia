//! Interactive trim mode: enter a non-modal editing state over the playing
//! video, mark one or more cut segments on a timeline, then hand them to the
//! video converter GUI, which does the export.
//!
//! - [`state`] — pure segment/mark logic (unit-tested).
//! - [`bar`] — the `VideoTrimBar` Relm4 component (timeline + controls).
//! - [`run`] — the shell-side glue (`impl VideoShell`): enter/exit, marking
//!   from the live mpv position, and the hand-off.

pub(super) mod bar;
mod run;
mod state;

pub(super) use bar::{VideoTrimBar, VideoTrimBarOutput};
