//! [`ConverterShell`](super::ConverterShell) message handlers: profile/GPU
//! restore, queue mutation, conversion start + progress drain, and audio/video
//! preview wiring. Split out of `shell.rs` (recipe: file-size budget); pure
//! move, behavior identical. Methods are `pub(super)` so the retained
//! `dispatch` (in `shell.rs`) can call them.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use gtk::gio;
use log::info;
use relm4::{ComponentController, ComponentSender};

use big_media_components::converter::header_bar::BigConverterHeaderInput;
use big_media_components::converter::progress::BigConverterProgressInput;
use big_media_components::converter::queue::BigConversionQueueStatus;
use big_media_components::converter::queue_component::BigConverterQueueInput;
use big_media_components::ffmpeg::progress::FfmpegProgressSnapshot;
use big_media_runtime::convertd::{
    ConversionEntryDetails, ConversionEntryState, ConversionJobDetails,
};
use big_os_kit::microdaemon::{DurableJobSnapshot, DurableJobState, JobProgress};

use crate::converter::daemon::{DaemonConversionRequest, DaemonJobAccepted, DaemonSubmitError};
use crate::converter::queue::{ConversionStatus, QueueMessage, SegmentOutputMode};
use crate::converter::settings::{self as conv_settings, keys};
use crate::converter::ui::sidebar::ConverterSidebarInput;
use crate::i18n::i18n;
use big_media_runtime::cast::gpu::{GpuDevice, GpuType};
use big_media_runtime::editor::audio_edit_page::AudioEditorInput;
use big_media_runtime::editor::edit_page::{VideoEditResult, VideoEditorInput};

use super::setup::to_queue_entry;
use super::{ConverterShell, ShellMessage};

#[derive(Default)]
struct DaemonQueueEntryApplyResult {
    is_running: bool,
    is_terminal: bool,
    current_file_name: Option<String>,
}

impl DaemonQueueEntryApplyResult {
    fn terminal() -> Self {
        Self {
            is_running: false,
            is_terminal: true,
            current_file_name: None,
        }
    }
}

impl ConverterShell {
    pub(super) fn load_saved_profile(&mut self) {
        let saved_out = self.settings.get_string(keys::OUTPUT_DIR);
        let default_out = if self.mode.is_audio() {
            big_os_kit::xdg_dirs::audio_dir().unwrap_or_else(std::env::temp_dir)
        } else {
            big_os_kit::xdg_dirs::video_dir().unwrap_or_else(std::env::temp_dir)
        };
        let output_dir = if saved_out.is_empty() {
            default_out
        } else {
            PathBuf::from(saved_out)
        };
        self.queue.set_output_dir(output_dir.clone());
        self.sidebar.emit(ConverterSidebarInput::SetOutputDir(
            output_dir.to_string_lossy().into_owned(),
        ));

        if self.mode.is_audio() {
            self.queue.set_audio_only_mode(true);
        } else {
            let last = self.settings.get_string(keys::LAST_PROFILE);
            if !last.is_empty() {
                if let Some(profile) = self.profiles.get(last.as_str()).cloned() {
                    self.sidebar
                        .emit(ConverterSidebarInput::ApplyProfile(Box::new(
                            profile.clone(),
                        )));
                    self.queue.set_profile(profile);
                }
            }
        }

        // GPU picker: stable English IDs for save/restore; translated labels shown.
        let device_labels: Vec<String> = self.gpus.iter().map(GpuDevice::label).collect();
        let mut gpu_ids: Vec<String> = vec!["Auto".to_string()];
        gpu_ids.extend(device_labels.iter().cloned());
        let mut gpu_labels: Vec<String> = vec![i18n("Auto")];
        gpu_labels.extend(device_labels);
        self.sidebar
            .emit(ConverterSidebarInput::SetGpus(gpu_labels));

        if !self.settings.get_bool(keys::GPU_AUTO_MIGRATED) {
            let _ = self.settings.set_boolean(keys::GPU_AUTO_MIGRATED.0, true);
            let _ = self.settings.set_string(keys::GPU_DEVICE.0, "Auto");
        }
        let saved_gpu = self.settings.get_string(keys::GPU_DEVICE);
        if !saved_gpu.is_empty() {
            if let Some(idx) = gpu_ids.iter().position(|l| l == saved_gpu.as_str()) {
                self.sidebar
                    .emit(ConverterSidebarInput::SetGpuSelected(idx as u32));
            }
        }
        self.gpu_ids = gpu_ids;

        let segment_mode = self.settings.get_string(keys::SEGMENT_MODE);
        if let Some(ve) = &self.video_editor {
            ve.emit(VideoEditorInput::SetSegmentModeSplit(
                segment_mode.as_str() == "split",
            ));
        }
    }

    pub(super) fn persist_gpu_device(&self, gpu_index: usize) {
        if let Some(id) = self.gpu_ids.get(gpu_index) {
            let _ = self.settings.set_string(keys::GPU_DEVICE.0, id);
        }
    }

    pub(super) fn add_files(&mut self, paths: &[PathBuf]) {
        for path in paths {
            self.add_file(path.clone());
        }
    }

    pub(super) fn add_file(&mut self, path: PathBuf) {
        let was_empty = self.queue.items().is_empty();

        // Lexical parent (dropped/picked paths are absolute) — avoids a
        // per-file `canonicalize` syscall on the main loop; LAST_DIR only needs
        // the folder the user added from, not the symlink-resolved target.
        if let Some(parent) = path.parent().and_then(Path::to_str) {
            conv_settings::converter_settings(self.mode).put_string(keys::LAST_DIR, parent);
        }

        let id = self.queue.add_file(path);
        if let Some(queue_item) = self.queue.items().iter().find(|i| i.id == id) {
            self.queue_view
                .emit(BigConverterQueueInput::AddEntry(to_queue_entry(queue_item)));
        }
        self.header.emit(BigConverterHeaderInput::SetQueueCount(
            self.queue.items().len(),
        ));

        if was_empty && self.mode.is_audio() {
            self.open_audio_preview(id, false);
        }
    }

    pub(super) fn start_conversion(&mut self, sender: &ComponentSender<ConverterShell>) {
        let Some(params) = self.params.clone() else {
            return;
        };

        if self.mode.is_audio() {
            if let Some(ae) = &self.audio_editor {
                ae.emit(AudioEditorInput::Stop);
            }
            self.queue_view
                .emit(BigConverterQueueInput::SetPlayingState(
                    self.current_audio_preview_item,
                    false,
                ));
        }

        self.queue.set_profile(params.profile.clone());

        if self.mode.is_audio() {
            let segment_mode = if params.audio_segment_split {
                SegmentOutputMode::Split
            } else {
                SegmentOutputMode::Join
            };
            self.queue.set_pending_segment_mode(segment_mode);
            match params.audio_cut_mode {
                0 => self.queue.clear_pending_segments(),
                1 => self.queue.sort_pending_segments_chronological(),
                _ => {}
            }
        }

        if params.gpu_index == 0 {
            let hw_gpus: Vec<GpuDevice> = self
                .gpus
                .iter()
                .filter(|g| g.gpu_type != GpuType::Cpu)
                .cloned()
                .collect();
            if hw_gpus.is_empty() {
                self.queue
                    .set_gpu(self.gpus.last().cloned().unwrap_or(GpuDevice {
                        gpu_type: GpuType::Cpu,
                        name: "CPU".into(),
                        device_path: String::new(),
                    }));
            } else {
                self.queue.set_gpus(hw_gpus);
            }
        } else if let Some(gpu) = self.gpus.get(params.gpu_index - 1) {
            self.queue.set_gpu(gpu.clone());
        }

        if self.queue.count(&ConversionStatus::Pending) == 0 {
            log::warn!("Convert requested with no pending items in the queue");
            return;
        }

        let queue_entry_paths: Vec<(u64, PathBuf)> = self
            .queue
            .items()
            .iter()
            .filter(|queue_entry| queue_entry.status == ConversionStatus::Pending)
            .map(|queue_entry| (queue_entry.id, queue_entry.input.clone()))
            .collect();
        info!("Starting conversion of {} files", queue_entry_paths.len());
        self.progress_total = queue_entry_paths.len();
        self.progress_done = 0;
        self.progress
            .emit(BigConverterProgressInput::SetConversions(queue_entry_paths));

        self.main_stack.set_visible_child_name("progress");
        self.queue.delete_source = params.delete_source;
        self.header
            .emit(BigConverterHeaderInput::SetConverting(true));
        self.converting = true;
        let request = DaemonConversionRequest {
            mode: self.mode,
            queue_entries: self
                .queue
                .items()
                .iter()
                .filter(|queue_item| queue_item.status == ConversionStatus::Pending)
                .cloned()
                .collect(),
            output_dir: self.queue.output_dir().to_path_buf(),
            profile: self.queue.profile().clone(),
            delete_source: self.queue.delete_source,
            audio_only_mode: self.queue.audio_only_mode,
        };
        let submit_sender = sender.clone();
        big_relm4_components::task::spawn_blocking_result(
            move || crate::converter::daemon::submit_conversion(request),
            move |result| submit_sender.input(ShellMessage::DaemonSubmitFinished(result)),
        );
    }

    pub(super) fn finish_daemon_submit(
        &mut self,
        result: Result<DaemonJobAccepted, DaemonSubmitError>,
        sender: &ComponentSender<ConverterShell>,
    ) {
        match result {
            Ok(accepted) => {
                if !self.converting {
                    cancel_daemon_job(sender, accepted.job_id);
                    return;
                }
                self.finish_daemon_handoff(accepted);
            }
            Err(error) => {
                if !self.converting {
                    return;
                }
                log::warn!(
                    "big-convertd submit failed; falling back to in-process conversion: {error}"
                );
                self.queue.start();
            }
        }
    }

    fn finish_daemon_handoff(&mut self, accepted: DaemonJobAccepted) {
        info!(
            "Conversion job {} handed off to big-convertd",
            accepted.job_id
        );
        if let Some(app) = self.window.application() {
            let notification = gio::Notification::new(&i18n("Conversion queued"));
            notification.set_body(Some(&i18n(
                "The conversion is running in the background service",
            )));
            app.send_notification(Some("conversion-daemon-queued"), &notification);
        }
        self.daemon_job = Some(accepted);
    }

    pub(super) fn finish_daemon_cancel(&mut self, result: Result<(), DaemonSubmitError>) {
        if let Err(error) = result {
            log::warn!("big-convertd cancel failed: {error}");
            return;
        }
        self.finish_daemon_queue_entries(
            DurableJobState::Cancelled,
            Some(i18n("Conversion cancelled")),
        );
    }

    pub(super) fn cancel_conversion(&mut self, sender: &ComponentSender<ConverterShell>) {
        if let Some(job) = &self.daemon_job {
            cancel_daemon_job(sender, job.job_id.clone());
            self.header
                .emit(BigConverterHeaderInput::SetConverting(false));
            self.converting = false;
            info!("Daemon conversion cancel requested by user");
            return;
        }
        self.queue.cancel();
        self.header
            .emit(BigConverterHeaderInput::SetConverting(false));
        self.converting = false;
        info!("Conversion cancelled by user");
    }

    #[allow(clippy::too_many_lines)] // queue-message drain — one arm per QueueMessage
    pub(super) fn tick_progress(&mut self, sender: &ComponentSender<ConverterShell>) {
        self.poll_daemon_snapshot(sender);
        let mut messages = Vec::new();
        while let Ok(msg) = self.queue.receiver().try_recv() {
            messages.push(msg);
        }
        for msg in messages {
            match msg {
                QueueMessage::Started {
                    entry_id: item_id,
                    command,
                } => {
                    self.progress
                        .emit(BigConverterProgressInput::SetCommand(item_id, command));
                }
                QueueMessage::Progress {
                    entry_id: item_id,
                    progress_snapshot,
                } => {
                    self.progress
                        .emit(BigConverterProgressInput::UpdateProgress(
                            item_id,
                            queue_progress_snapshot(progress_snapshot),
                        ));
                    self.queue_view.emit(BigConverterQueueInput::UpdateStatus(
                        item_id,
                        BigConversionQueueStatus::Converting,
                    ));
                    if self.queue.gpu_count() <= 1 {
                        if let Some(name) = self
                            .queue
                            .items()
                            .iter()
                            .find(|i| i.id == item_id)
                            .and_then(|i| i.input.file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                        {
                            self.progress
                                .emit(BigConverterProgressInput::SetCurrentFile(name));
                        }
                    } else {
                        self.progress
                            .emit(BigConverterProgressInput::SetCurrentParallel(
                                self.queue.gpu_count(),
                            ));
                    }
                    let mode_label = i18n(if self.mode.is_audio() {
                        "Converting Audio"
                    } else {
                        "Converting Videos"
                    });
                    let shown = (self.progress_done + 1).min(self.progress_total.max(1));
                    self.window.set_title(Some(&format!(
                        "{mode_label} ({shown}/{})",
                        self.progress_total
                    )));
                }
                QueueMessage::Done { entry_id: item_id } => {
                    self.progress
                        .emit(BigConverterProgressInput::MarkDone(item_id));
                    self.queue_view
                        .emit(BigConverterQueueInput::RemoveEntry(item_id));
                    self.queue.remove_entry(item_id);
                    self.progress_done += 1;
                    self.header.emit(BigConverterHeaderInput::SetQueueCount(
                        self.queue.items().len(),
                    ));
                }
                QueueMessage::Failed {
                    entry_id: item_id,
                    error,
                } => {
                    self.progress.emit(BigConverterProgressInput::MarkFailed(
                        item_id,
                        error.clone(),
                    ));
                    self.queue_view.emit(BigConverterQueueInput::UpdateStatus(
                        item_id,
                        BigConversionQueueStatus::Failed,
                    ));
                    self.progress_done += 1;
                }
                QueueMessage::Cancelled { entry_id: item_id } => {
                    self.progress
                        .emit(BigConverterProgressInput::MarkCancelled(item_id));
                    self.queue_view.emit(BigConverterQueueInput::UpdateStatus(
                        item_id,
                        BigConversionQueueStatus::Cancelled,
                    ));
                    self.progress_done += 1;
                }
                QueueMessage::AllDone => {
                    self.header
                        .emit(BigConverterHeaderInput::SetConverting(false));
                    self.converting = false;
                    self.progress.emit(BigConverterProgressInput::ShowComplete);
                    self.window.set_title(Some(&i18n(if self.mode.is_audio() {
                        "Audio Converter"
                    } else {
                        "Video Converter"
                    })));
                    info!("All conversions complete");
                    if !self.window.is_active() {
                        if let Some(app) = self.window.application() {
                            let notif = gio::Notification::new(&i18n("Conversion complete"));
                            notif.set_body(Some(&i18n("All files have been converted")));
                            app.send_notification(Some("conversion-done"), &notif);
                        }
                    }
                }
                QueueMessage::GpuFallback {
                    entry_id: item_id,
                    stage,
                } => {
                    info!("GPU fallback for item {item_id}: {stage}");
                }
                QueueMessage::StderrLog {
                    entry_id: item_id,
                    lines,
                } => {
                    self.progress
                        .emit(BigConverterProgressInput::SetStderrLog(item_id, lines));
                }
                QueueMessage::Probed {
                    entry_id: item_id, ..
                } => {
                    // Model fields were updated by queue.process_messages;
                    // the row does not render duration, so no view update.
                    log::debug!("probe finished for item {item_id}");
                }
                QueueMessage::StderrLine {
                    entry_id: item_id,
                    line,
                } => {
                    self.progress
                        .emit(BigConverterProgressInput::AppendStderrLine(item_id, line));
                }
            }
        }
    }

    fn poll_daemon_snapshot(&mut self, sender: &ComponentSender<ConverterShell>) {
        let Some(job) = &self.daemon_job else {
            return;
        };
        if self.daemon_poll_in_flight {
            return;
        }
        self.daemon_poll_in_flight = true;
        let job_id = job.job_id.clone();
        let snapshot_sender = sender.clone();
        big_relm4_components::task::spawn_blocking_result(
            move || crate::converter::daemon::attach_conversion(job_id),
            move |result| snapshot_sender.input(ShellMessage::DaemonSnapshot(result)),
        );
    }

    pub(super) fn apply_daemon_snapshot(
        &mut self,
        result: Result<DurableJobSnapshot, DaemonSubmitError>,
        _sender: &ComponentSender<ConverterShell>,
    ) {
        self.daemon_poll_in_flight = false;
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(error) => {
                log::warn!("big-convertd attach failed: {error}");
                return;
            }
        };
        if self.daemon_job.as_ref().map(|job| job.job_id.as_str()) != Some(snapshot.job_id.as_str())
        {
            return;
        }
        let has_queue_entry_details =
            self.apply_daemon_queue_entry_details(snapshot.details.as_ref());
        match snapshot.state {
            DurableJobState::Queued | DurableJobState::Running | DurableJobState::Cancelling => {
                if !has_queue_entry_details {
                    self.apply_daemon_progress(snapshot.progress);
                }
            }
            DurableJobState::Completed => {
                self.finish_daemon_queue_entries(DurableJobState::Completed, snapshot.status_text);
            }
            DurableJobState::Cancelled => {
                self.finish_daemon_queue_entries(DurableJobState::Cancelled, snapshot.status_text);
            }
            DurableJobState::Interrupted | DurableJobState::Failed => {
                self.finish_daemon_queue_entries(DurableJobState::Failed, snapshot.status_text);
            }
        }
    }

    fn apply_daemon_progress(&mut self, progress: Option<JobProgress>) {
        let Some(job) = &self.daemon_job else {
            return;
        };
        let Some(first_queue_entry_id) = job.queue_entry_ids.first().copied() else {
            return;
        };
        if let Some(progress) = progress {
            self.progress
                .emit(BigConverterProgressInput::UpdateProgress(
                    first_queue_entry_id,
                    daemon_progress_snapshot(progress),
                ));
        }
        self.progress
            .emit(BigConverterProgressInput::SetCurrentParallel(1));
    }

    fn apply_daemon_queue_entry_details(&mut self, details: Option<&serde_json::Value>) -> bool {
        let Some(job) = &self.daemon_job else {
            return false;
        };
        let queue_entry_ids = job.queue_entry_ids.clone();
        let Some(details) = details
            .and_then(|value| serde_json::from_value::<ConversionJobDetails>(value.clone()).ok())
        else {
            return false;
        };
        if details.schema_version != 1 {
            return false;
        }

        let mut running_queue_entries = 0_usize;
        let mut terminal_queue_entries = 0_usize;
        let mut current_file_name = None;
        for daemon_queue_entry in details.items {
            let Some(local_queue_entry_id) =
                queue_entry_ids.get(daemon_queue_entry.entry_index).copied()
            else {
                continue;
            };
            let apply_result =
                self.apply_daemon_queue_entry_detail(local_queue_entry_id, daemon_queue_entry);
            running_queue_entries += usize::from(apply_result.is_running);
            terminal_queue_entries += usize::from(apply_result.is_terminal);
            if let Some(file_name) = apply_result.current_file_name {
                current_file_name = Some(file_name);
            }
        }

        self.progress_done = terminal_queue_entries.min(self.progress_total);
        if running_queue_entries > 1 {
            self.progress
                .emit(BigConverterProgressInput::SetCurrentParallel(
                    running_queue_entries,
                ));
        } else if let Some(name) = current_file_name {
            self.progress
                .emit(BigConverterProgressInput::SetCurrentFile(name));
        }
        if self.converting {
            let mode_label = i18n(if self.mode.is_audio() {
                "Converting Audio"
            } else {
                "Converting Videos"
            });
            let shown = (terminal_queue_entries + usize::from(running_queue_entries > 0))
                .min(self.progress_total.max(1));
            self.window.set_title(Some(&format!(
                "{mode_label} ({shown}/{})",
                self.progress_total
            )));
        }
        true
    }

    fn apply_daemon_queue_entry_detail(
        &mut self,
        local_queue_entry_id: u64,
        daemon_queue_entry: ConversionEntryDetails,
    ) -> DaemonQueueEntryApplyResult {
        if let Some(command) = daemon_queue_entry.command {
            self.progress.emit(BigConverterProgressInput::SetCommand(
                local_queue_entry_id,
                command,
            ));
        }
        if !daemon_queue_entry.stderr_tail.is_empty() {
            self.progress.emit(BigConverterProgressInput::SetStderrLog(
                local_queue_entry_id,
                daemon_queue_entry.stderr_tail,
            ));
        }
        match daemon_queue_entry.state {
            ConversionEntryState::Pending => DaemonQueueEntryApplyResult::default(),
            ConversionEntryState::Running => {
                if let Some(progress) = daemon_queue_entry.progress {
                    self.progress
                        .emit(BigConverterProgressInput::UpdateProgress(
                            local_queue_entry_id,
                            queue_progress_snapshot(progress),
                        ));
                }
                self.queue_view.emit(BigConverterQueueInput::UpdateStatus(
                    local_queue_entry_id,
                    BigConversionQueueStatus::Converting,
                ));
                DaemonQueueEntryApplyResult {
                    is_running: true,
                    is_terminal: false,
                    current_file_name: daemon_queue_entry
                        .input
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned()),
                }
            }
            ConversionEntryState::Completed => {
                self.progress
                    .emit(BigConverterProgressInput::MarkDone(local_queue_entry_id));
                DaemonQueueEntryApplyResult::terminal()
            }
            ConversionEntryState::Cancelled => {
                self.progress.emit(BigConverterProgressInput::MarkCancelled(
                    local_queue_entry_id,
                ));
                self.queue_view.emit(BigConverterQueueInput::UpdateStatus(
                    local_queue_entry_id,
                    BigConversionQueueStatus::Cancelled,
                ));
                DaemonQueueEntryApplyResult::terminal()
            }
            ConversionEntryState::Interrupted | ConversionEntryState::Failed => {
                self.progress.emit(BigConverterProgressInput::MarkFailed(
                    local_queue_entry_id,
                    daemon_failure_status_text(daemon_queue_entry.status_text),
                ));
                self.queue_view.emit(BigConverterQueueInput::UpdateStatus(
                    local_queue_entry_id,
                    BigConversionQueueStatus::Failed,
                ));
                DaemonQueueEntryApplyResult::terminal()
            }
        }
    }

    fn finish_daemon_queue_entries(&mut self, state: DurableJobState, status_text: Option<String>) {
        let Some(job) = self.daemon_job.take() else {
            return;
        };
        self.daemon_poll_in_flight = false;
        for queue_entry_id in job.queue_entry_ids {
            match state {
                DurableJobState::Completed => {
                    self.progress
                        .emit(BigConverterProgressInput::MarkDone(queue_entry_id));
                    self.queue_view
                        .emit(BigConverterQueueInput::RemoveEntry(queue_entry_id));
                    self.queue.remove_entry(queue_entry_id);
                }
                DurableJobState::Cancelled => {
                    self.progress
                        .emit(BigConverterProgressInput::MarkCancelled(queue_entry_id));
                    self.queue_view.emit(BigConverterQueueInput::UpdateStatus(
                        queue_entry_id,
                        BigConversionQueueStatus::Cancelled,
                    ));
                }
                DurableJobState::Failed => {
                    self.progress.emit(BigConverterProgressInput::MarkFailed(
                        queue_entry_id,
                        status_text
                            .clone()
                            .unwrap_or_else(|| i18n("Conversion failed")),
                    ));
                    self.queue_view.emit(BigConverterQueueInput::UpdateStatus(
                        queue_entry_id,
                        BigConversionQueueStatus::Failed,
                    ));
                }
                DurableJobState::Queued
                | DurableJobState::Running
                | DurableJobState::Cancelling
                | DurableJobState::Interrupted => {}
            }
        }
        self.header
            .emit(BigConverterHeaderInput::SetConverting(false));
        self.header.emit(BigConverterHeaderInput::SetQueueCount(
            self.queue.items().len(),
        ));
        self.converting = false;
        self.progress.emit(BigConverterProgressInput::ShowComplete);
        self.window.set_title(Some(&i18n(if self.mode.is_audio() {
            "Audio Converter"
        } else {
            "Video Converter"
        })));
    }

    pub(super) fn clear_queue(&mut self) {
        self.queue.clear_all();
        self.queue_view.emit(BigConverterQueueInput::Clear);
        self.queue_view
            .emit(BigConverterQueueInput::SetPlayingEntry(None));
        self.current_audio_preview_item = None;
        self.header.emit(BigConverterHeaderInput::SetQueueCount(0));
        self.audio_preview_revealer.set_reveal_child(false);
        if let Some(ae) = &self.audio_editor {
            ae.emit(AudioEditorInput::Unload);
        }
        self.window.set_title(Some(&i18n(if self.mode.is_audio() {
            "Audio Converter"
        } else {
            "Video Converter"
        })));
    }

    pub(super) fn remove_queue_entry(&mut self, item_id: u64) {
        self.queue.remove_entry(item_id);
        if self.current_audio_preview_item == Some(item_id) {
            self.current_audio_preview_item = None;
            self.queue_view
                .emit(BigConverterQueueInput::SetPlayingEntry(None));
            self.audio_preview_revealer.set_reveal_child(false);
            if let Some(ae) = &self.audio_editor {
                ae.emit(AudioEditorInput::Unload);
            }
        }
        self.header.emit(BigConverterHeaderInput::SetQueueCount(
            self.queue.items().len(),
        ));
    }

    pub(super) fn on_queue_play(&mut self, path: &Path) {
        if self.mode.is_audio() {
            if let Some(queue_entry_id) = self.queue_entry_id_for_path(path) {
                self.open_audio_preview(queue_entry_id, true);
                return;
            }
        }
        big_app_kit::desktop::open_file(path);
    }

    pub(super) fn on_queue_select(&mut self, path: &Path) {
        if self.mode.is_audio() {
            if let Some(queue_entry_id) = self.queue_entry_id_for_path(path) {
                self.open_audio_preview(queue_entry_id, false);
            }
        }
    }

    pub(super) fn on_queue_edit(&mut self, path: &Path) {
        let Some(ve) = &self.video_editor else { return };
        self.editing_item_id = self.queue_entry_id_for_path(path);
        ve.emit(VideoEditorInput::LoadFile(path.to_path_buf()));
        self.main_stack.set_visible_child_name("edit");
    }

    pub(super) fn open_audio_preview(&mut self, item_id: u64, autoplay: bool) {
        let Some(ae) = &self.audio_editor else { return };
        let Some(queue_item) = self.queue.items().iter().find(|i| i.id == item_id) else {
            return;
        };
        let input_path = queue_item.input.clone();
        let segments = queue_item.segments.clone();

        ae.emit(AudioEditorInput::LoadFile(input_path));
        ae.emit(AudioEditorInput::SetSegments(segments));
        self.current_audio_preview_item = Some(item_id);
        self.queue_view
            .emit(BigConverterQueueInput::SetPlayingState(
                Some(item_id),
                autoplay,
            ));
        self.audio_preview_revealer.set_reveal_child(true);
        if autoplay {
            ae.emit(AudioEditorInput::Play);
        }
    }

    pub(super) fn step_audio_track(&mut self, delta: i64) {
        let Some(current) = self.current_audio_preview_item else {
            return;
        };
        let next_id = {
            let queue_items = self.queue.items();
            let Some(idx) = queue_items
                .iter()
                .position(|queue_item| queue_item.id == current)
            else {
                return;
            };
            let target = idx as i64 + delta;
            if target < 0 {
                None
            } else {
                queue_items
                    .get(target as usize)
                    .map(|queue_item| queue_item.id)
            }
        };
        if let Some(id) = next_id {
            self.open_audio_preview(id, true);
        }
    }

    pub(super) fn on_audio_segments_applied(
        &mut self,
        ranges: Vec<crate::converter::ffmpeg::TrimRange>,
    ) {
        let Some(item_id) = self.current_audio_preview_item else {
            return;
        };
        let non_empty = !ranges.is_empty();
        if let Some(qi) = self.queue.entry_mut(item_id) {
            qi.segments = ranges;
        }
        // Auto-enable cut mode when the user first marks a segment, else the
        // marked segments are silently dropped at conversion time.
        if non_empty {
            self.sidebar.emit(ConverterSidebarInput::EnsureCutMode);
        }
    }

    pub(super) fn on_audio_apply_and_convert(
        &mut self,
        ranges: Vec<crate::converter::ffmpeg::TrimRange>,
        sender: &ComponentSender<ConverterShell>,
    ) {
        if let Some(item_id) = self.current_audio_preview_item {
            if let Some(qi) = self.queue.entry_mut(item_id) {
                qi.segments = ranges;
            }
        }
        self.audio_preview_revealer.set_reveal_child(false);
        if let Some(ae) = &self.audio_editor {
            ae.emit(AudioEditorInput::Stop);
        }
        self.start_conversion(sender);
    }

    pub(super) fn apply_video_edits_and_convert(
        &mut self,
        result: VideoEditResult,
        sender: &ComponentSender<ConverterShell>,
    ) {
        if let Some(item_id) = self.editing_item_id {
            let segment_mode = if result.segment_mode_split {
                SegmentOutputMode::Split
            } else {
                SegmentOutputMode::Join
            };
            self.settings.put_string(
                keys::SEGMENT_MODE,
                if result.segment_mode_split {
                    "split"
                } else {
                    "join"
                },
            );
            self.settings.flush();
            if let Some(qi) = self.queue.entry_mut(item_id) {
                qi.segments = result.ranges;
                qi.video_overrides = Some(result.overrides);
                qi.crop = result.crop;
                qi.segment_mode = segment_mode;
            }
        }
        if let Some(ve) = &self.video_editor {
            ve.emit(VideoEditorInput::Unload);
        }
        self.main_stack.set_visible_child_name("main");
        self.start_conversion(sender);
    }

    pub(super) fn show_file_metadata(&self, item_id: u64) {
        if let Some(path) = self
            .queue
            .items()
            .iter()
            .find(|queue_item| queue_item.id == item_id)
            .map(|queue_item| queue_item.input.clone())
        {
            crate::converter::ui::dialogs::file_metadata_dialog::show_file_metadata_dialog(
                self.window.upcast_ref::<gtk::Window>(),
                &path,
            );
        }
    }

    pub(super) fn queue_entry_id_for_path(&self, path: &Path) -> Option<u64> {
        self.queue
            .items()
            .iter()
            .find(|queue_item| queue_item.input == path)
            .map(|queue_item| queue_item.id)
    }
}

fn cancel_daemon_job(sender: &ComponentSender<ConverterShell>, job_id: String) {
    let cancel_sender = sender.clone();
    big_relm4_components::task::spawn_blocking_result(
        move || crate::converter::daemon::cancel_conversion(job_id),
        move |result| cancel_sender.input(ShellMessage::DaemonCancelFinished(result)),
    );
}

fn daemon_failure_status_text(status_text: Option<String>) -> String {
    status_text.unwrap_or_else(|| i18n("Conversion failed"))
}

fn daemon_progress_snapshot(progress: JobProgress) -> FfmpegProgressSnapshot {
    let percent = progress.total_units.map_or(0.0, |total| {
        if total == 0 {
            0.0
        } else {
            progress.completed_units as f64 / total as f64 * 100.0
        }
    });
    FfmpegProgressSnapshot {
        percent: percent.clamp(0.0, 100.0),
        time_secs: percent,
        total_secs: 100.0,
        ..Default::default()
    }
}

fn queue_progress_snapshot(
    runtime_progress_snapshot: big_media_runtime::converter::ffmpeg::progress::FfmpegProgressSnapshot,
) -> FfmpegProgressSnapshot {
    // Single bridge impl lives in `crate::export` so the player embedding and the
    // converter share one runtime→UI FfmpegProgressSnapshot conversion.
    crate::export::ui_progress_snapshot(&runtime_progress_snapshot)
}

#[cfg(test)]
mod daemon_progress_tests {
    use super::*;

    #[test]
    fn daemon_progress_snapshot_clamps_to_percent_units() {
        let progress_snapshot = daemon_progress_snapshot(JobProgress::new(250, Some(100)));

        assert_eq!(progress_snapshot.percent, 100.0);
        assert_eq!(progress_snapshot.total_secs, 100.0);
    }

    #[test]
    fn daemon_progress_snapshot_handles_unknown_total() {
        let progress_snapshot = daemon_progress_snapshot(JobProgress::new(5, None));

        assert_eq!(progress_snapshot.percent, 0.0);
    }

    #[test]
    fn queue_progress_snapshot_preserves_runtime_fields_for_ui() {
        let progress_snapshot = queue_progress_snapshot(
            big_media_runtime::converter::ffmpeg::progress::FfmpegProgressSnapshot {
                time_secs: 2.0,
                total_secs: 10.0,
                speed: 1.5,
                size_bytes: 2048,
                bitrate_kbps: 128.0,
                frame: 42,
                fps: 24.0,
                eta_secs: 8.0,
                percent: 20.0,
                done: true,
            },
        );

        assert_eq!(progress_snapshot.time_secs, 2.0);
        assert_eq!(progress_snapshot.size_bytes, 2048);
        assert_eq!(progress_snapshot.percent, 20.0);
        assert!(progress_snapshot.done);
    }
}
