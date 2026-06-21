//! Shell-side trim glue (`impl VideoShell`): enter/exit trim mode, mark cuts at
//! the live mpv position, and drive the export through the shared conversion
//! queue while streaming progress into the shared `BigConverterProgress` page.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use gtk::glib;
use relm4::{ComponentController, ComponentSender};

use big_converter_ui::export::{
    build_single_export_queue, ui_progress_info, QuickExportChoice, QuickExportFormat,
};
use big_media_components::converter::progress::{
    BigConverterProgressInput as ProgIn, BigConverterProgressOutput as ProgOut,
};
use big_media_runtime::converter::profiles::VideoFilters;
use big_media_runtime::converter::queue::{QueueMessage, SegmentOutputMode};

use crate::i18n::i18n;

use super::super::{VideoShell, VideoShellInput};
use super::bar::{VideoTrimBarInput, VideoTrimBarOutput};
use super::state::CutMode;
use super::TrimMarkKind;

impl VideoShell {
    /// Enter the interactive trim mode (no-op without a local file playing).
    pub(in crate::video::ui::video_shell) fn enter_trim_mode(&mut self) {
        if self.current_local_input().is_none() {
            self.notify(&i18n("Open a local video to cut it"));
            return;
        }
        self.trim_active.set(true);
        // Pin the OSD so the transport (scrub/play) stays available while cutting
        // (the OSD shares this same flag).
        self.osd.show();
        self.trim_revealer.set_reveal_child(true);
        self.trim
            .emit(VideoTrimBarInput::SetDuration(self.trim_duration()));
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            self.trim.emit(VideoTrimBarInput::SetPlayhead(
                mpv.get_property_f64("time-pos"),
            ));
        }
    }

    /// Leave trim mode and restore the normal OSD auto-hide.
    pub(in crate::video::ui::video_shell) fn exit_trim_mode(&mut self) {
        self.trim_active.set(false);
        self.trim_revealer.set_reveal_child(false);
        self.osd.show_and_schedule_hide();
    }

    /// Push the live playhead to the trim bar (called from the position tick
    /// while trim mode is active).
    pub(in crate::video::ui::video_shell) fn feed_trim_playhead(&self, pos: f64) {
        if self.trim_active.get() {
            self.trim.emit(VideoTrimBarInput::SetPlayhead(pos));
        }
    }

    /// Push a new duration to the trim bar (called on `DurationChanged`).
    pub(in crate::video::ui::video_shell) fn feed_trim_duration(&self, dur: f64) {
        if self.trim_active.get() {
            self.trim.emit(VideoTrimBarInput::SetDuration(dur));
        }
    }

    /// Keyboard mark: read mpv's exact `time-pos` and mark in/out/quick there.
    pub(in crate::video::ui::video_shell) fn trim_mark(&self, kind: TrimMarkKind) {
        if !self.trim_active.get() {
            return;
        }
        let t = self
            .mpv
            .borrow()
            .as_ref()
            .map_or(0.0, |mpv| mpv.get_property_f64("time-pos"));
        self.trim.emit(match kind {
            TrimMarkKind::In => VideoTrimBarInput::MarkIn(t),
            TrimMarkKind::Out => VideoTrimBarInput::MarkOut(t),
            TrimMarkKind::Quick => VideoTrimBarInput::QuickMark(t),
        });
    }

    /// Act on a typed effect from the trim bar.
    pub(in crate::video::ui::video_shell) fn on_trim_bar(
        &mut self,
        out: VideoTrimBarOutput,
        sender: &ComponentSender<Self>,
    ) {
        match out {
            VideoTrimBarOutput::SeekPreview(pos) => {
                if let Some(mpv) = self.mpv.borrow().as_ref() {
                    mpv.seek_absolute(pos);
                }
            }
            VideoTrimBarOutput::Exit => self.exit_trim_mode(),
            VideoTrimBarOutput::ExportRequested {
                ranges,
                mode,
                split,
            } => self.start_trim_export(&ranges, mode, split, sender),
        }
    }

    /// Build + start the clip export, show the progress page, and stream queue
    /// messages into it until completion.
    fn start_trim_export(
        &mut self,
        ranges: &[(f64, f64)],
        mode: CutMode,
        split: bool,
        sender: &ComponentSender<Self>,
    ) {
        let Some(input) = self.current_local_input() else {
            self.notify(&i18n("Open a local video to cut it"));
            return;
        };
        let output_dir = input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

        let choice = match mode {
            CutMode::Fast => QuickExportChoice::keep_format(&input),
            CutMode::Precise => QuickExportChoice {
                format: QuickExportFormat::Video {
                    container: input
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(str::to_ascii_lowercase)
                        .filter(|ext| !ext.is_empty())
                        .unwrap_or_else(|| "mp4".to_string()),
                    codec: "h264".into(),
                    quality: "good".into(),
                },
                filters: VideoFilters::default(),
            },
        };

        let (mut queue, id, output) =
            build_single_export_queue(input.clone(), output_dir, &choice, ranges);
        if output.as_os_str().is_empty() {
            self.notify(&i18n("Export failed"));
            return;
        }
        if split {
            queue.set_pending_segment_mode(SegmentOutputMode::Split);
        }

        // Progress page: one row for this file.
        self.trim_progress.emit(ProgIn::Reset);
        self.trim_progress
            .emit(ProgIn::SetItems(vec![(id, input.clone())]));
        self.trim_progress.emit(ProgIn::SetCurrentFile(
            input
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string(),
        ));
        self.trim_progress_overlay.set_visible(true);
        self.trim.emit(VideoTrimBarInput::SetBusy(true));

        queue.start();
        let receiver = queue.receiver().clone();
        *self.trim_queue.borrow_mut() = Some(queue);

        let progress = self.trim_progress.sender().clone();
        let shell = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let mut outcome: TrimExportOutcome = TrimExportOutcome::Ok;
            while let Ok(msg) = receiver.recv().await {
                match msg {
                    QueueMessage::Started { item_id, command } => {
                        let _ = progress.send(ProgIn::SetCommand(item_id, command));
                    }
                    QueueMessage::Progress { item_id, info } => {
                        let _ =
                            progress.send(ProgIn::UpdateProgress(item_id, ui_progress_info(&info)));
                    }
                    QueueMessage::StderrLine { item_id, line } => {
                        let _ = progress.send(ProgIn::AppendStderrLine(item_id, line));
                    }
                    QueueMessage::Done { item_id } => {
                        let _ = progress.send(ProgIn::MarkDone(item_id));
                    }
                    QueueMessage::Failed { item_id, error } => {
                        let _ = progress.send(ProgIn::MarkFailed(item_id, error.clone()));
                        outcome = TrimExportOutcome::Failed(error);
                    }
                    QueueMessage::Cancelled { item_id } => {
                        let _ = progress.send(ProgIn::MarkCancelled(item_id));
                        outcome = TrimExportOutcome::Cancelled;
                    }
                    QueueMessage::AllDone => {
                        let _ = progress.send(ProgIn::ShowComplete);
                        break;
                    }
                    _ => {}
                }
            }
            let _ = shell.send(VideoShellInput::TrimExportFinished(outcome));
        });
    }

    /// React to the progress page's own buttons (Back dismisses; Cancel aborts).
    pub(in crate::video::ui::video_shell) fn on_trim_progress(&mut self, out: ProgOut) {
        match out {
            ProgOut::Back => self.trim_progress_overlay.set_visible(false),
            ProgOut::CancelAll | ProgOut::CancelItem(_) => {
                if let Some(queue) = self.trim_queue.borrow().as_ref() {
                    queue.cancel();
                }
            }
        }
    }

    /// Finish an export: re-enable the bar, drop the queue, toast the outcome.
    pub(in crate::video::ui::video_shell) fn finish_trim_export(
        &mut self,
        outcome: TrimExportOutcome,
    ) {
        self.trim.emit(VideoTrimBarInput::SetBusy(false));
        *self.trim_queue.borrow_mut() = None;
        match outcome {
            TrimExportOutcome::Ok => self.notify(&i18n("Clip exported")),
            TrimExportOutcome::Cancelled => self.notify(&i18n("Export cancelled")),
            TrimExportOutcome::Failed(error) => {
                self.notify(&format!("{}: {error}", i18n("Export failed")));
            }
        }
        // The progress page keeps its completion banner up; the user dismisses it
        // with its Back button (→ on_trim_progress hides the overlay).
    }

    /// Cancel any in-flight export (used on teardown so a closing window does not
    /// leave an ffmpeg worker running).
    pub(in crate::video::ui::video_shell) fn cancel_trim_export(&self) {
        if let Some(queue) = self.trim_queue.borrow().as_ref() {
            queue.cancel();
        }
    }

    /// The current local (non-stream) input path, or `None`.
    fn current_local_input(&self) -> Option<PathBuf> {
        let path = self
            .mpv
            .borrow()
            .as_ref()
            .and_then(|mpv| mpv.get_current_file())?;
        if super::super::playback::is_remote_uri(&path) {
            return None;
        }
        Some(PathBuf::from(path))
    }

    /// Best-known duration (mpv `duration` falls back when the cached value is
    /// not yet populated).
    fn trim_duration(&self) -> f64 {
        if self.duration_secs > 1.0 {
            self.duration_secs
        } else {
            self.mpv
                .borrow()
                .as_ref()
                .map_or(0.0, |mpv| mpv.get_property_f64("duration"))
                .max(1.0)
        }
    }
}

/// Terminal state of a clip export, carried back to the shell. Public because it
/// travels in the public [`VideoShellInput`] enum.
#[derive(Debug)]
pub enum TrimExportOutcome {
    /// Completed successfully.
    Ok,
    /// The user cancelled.
    Cancelled,
    /// FFmpeg / queue failed with this message.
    Failed(String),
}
