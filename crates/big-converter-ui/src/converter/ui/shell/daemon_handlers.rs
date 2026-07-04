//! big-convertd handoff, recovery, snapshot application, and progress helpers.
//! Split from handlers.rs to keep the shell message handlers navigable.

use std::path::PathBuf;

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

use crate::converter::daemon::{DaemonJobAccepted, DaemonSubmitError};
use crate::i18n::i18n;

use super::dialogs;
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
    pub(super) fn load_daemon_jobs(sender: &ComponentSender<ConverterShell>) {
        let list_sender = sender.clone();
        big_relm4_components::task::spawn_blocking_result(
            crate::converter::daemon::list_conversions,
            move |result| list_sender.input(ShellMessage::DaemonListFinished(result)),
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

    pub(super) fn finish_daemon_list(
        &mut self,
        result: Result<Vec<DurableJobSnapshot>, DaemonSubmitError>,
        sender: &ComponentSender<ConverterShell>,
    ) {
        let snapshots = match result {
            Ok(snapshots) => snapshots,
            Err(error) => {
                log::debug!("big-convertd startup list failed: {error}");
                return;
            }
        };
        if self.converting || self.daemon_job.is_some() {
            return;
        }
        let Some(snapshot) = select_recoverable_snapshot(self.mode, snapshots) else {
            return;
        };
        self.resume_daemon_snapshot(snapshot, sender);
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

    pub(super) fn restart_daemon_job(job_id: String, sender: &ComponentSender<ConverterShell>) {
        let restart_sender = sender.clone();
        big_relm4_components::task::spawn_blocking_result(
            move || crate::converter::daemon::restart_conversion(job_id),
            move |result| restart_sender.input(ShellMessage::DaemonRestartFinished(result)),
        );
    }

    pub(super) fn finish_daemon_restart(
        &mut self,
        result: Result<DurableJobSnapshot, DaemonSubmitError>,
        sender: &ComponentSender<ConverterShell>,
    ) {
        match result {
            Ok(snapshot) => self.resume_daemon_snapshot(snapshot, sender),
            Err(error) => {
                log::warn!("big-convertd restart failed: {error}");
            }
        }
    }

    pub(super) fn cleanup_daemon_job(job_id: String, sender: &ComponentSender<ConverterShell>) {
        let cleanup_sender = sender.clone();
        big_relm4_components::task::spawn_blocking_result(
            move || crate::converter::daemon::ack_cleanup_conversion(job_id),
            move |result| cleanup_sender.input(ShellMessage::DaemonCleanupFinished(result)),
        );
    }

    pub(super) fn forget_daemon_job(
        &mut self,
        job_id: String,
        sender: &ComponentSender<ConverterShell>,
    ) {
        if self.daemon_cleanup_job_id.as_deref() == Some(job_id.as_str()) {
            self.daemon_cleanup_job_id = None;
            self.reset_progress_page_to_queue();
        }
        Self::cleanup_daemon_job(job_id, sender);
    }

    pub(super) fn finish_daemon_cleanup(result: Result<(), DaemonSubmitError>) {
        if let Err(error) = result {
            log::warn!("big-convertd cleanup failed: {error}");
        }
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

    pub(super) fn return_to_queue_from_progress(
        &mut self,
        sender: &ComponentSender<ConverterShell>,
    ) {
        if let Some(job_id) = self.daemon_cleanup_job_id.take() {
            Self::cleanup_daemon_job(job_id, sender);
        }
        self.reset_progress_page_to_queue();
    }

    fn reset_progress_page_to_queue(&mut self) {
        self.queue.reset_non_done();
        self.queue_view
            .emit(BigConverterQueueInput::ResetStatusIcons);
        self.main_stack.set_visible_child_name("main");
        self.progress.emit(BigConverterProgressInput::Reset);
    }

    pub(super) fn poll_daemon_snapshot(&mut self, sender: &ComponentSender<ConverterShell>) {
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
        sender: &ComponentSender<ConverterShell>,
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
        let job_id = snapshot.job_id.clone();
        let status_text = snapshot.status_text.clone();
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
                dialogs::show_daemon_recovery_dialog(&self.window, sender, job_id, status_text);
            }
        }
    }

    fn resume_daemon_snapshot(
        &mut self,
        snapshot: DurableJobSnapshot,
        sender: &ComponentSender<ConverterShell>,
    ) {
        let Some(queue_entry_ids) = daemon_snapshot_queue_entry_ids(&snapshot) else {
            log::warn!(
                "big-convertd job {} has no restorable conversion details",
                snapshot.job_id
            );
            return;
        };
        let Some(progress_rows) = daemon_snapshot_progress_rows(&snapshot) else {
            log::warn!(
                "big-convertd job {} has no restorable progress rows",
                snapshot.job_id
            );
            return;
        };
        self.progress_total = progress_rows.len();
        self.progress_done = 0;
        self.progress
            .emit(BigConverterProgressInput::SetConversions(progress_rows));
        self.main_stack.set_visible_child_name("progress");
        self.daemon_job = Some(DaemonJobAccepted {
            job_id: snapshot.job_id.clone(),
            queue_entry_ids,
        });
        let is_active = daemon_snapshot_is_active(snapshot.state);
        self.converting = is_active;
        self.header
            .emit(BigConverterHeaderInput::SetConverting(is_active));
        self.apply_daemon_snapshot(Ok(snapshot), sender);
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
        let job_id = job.job_id.clone();
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
        self.daemon_cleanup_job_id = Some(job_id);
        self.window.set_title(Some(&i18n(if self.mode.is_audio() {
            "Audio Converter"
        } else {
            "Video Converter"
        })));
    }
}

fn cancel_daemon_job(sender: &ComponentSender<ConverterShell>, job_id: String) {
    let cancel_sender = sender.clone();
    big_relm4_components::task::spawn_blocking_result(
        move || crate::converter::daemon::cancel_conversion(job_id),
        move |result| cancel_sender.input(ShellMessage::DaemonCancelFinished(result)),
    );
}

fn select_recoverable_snapshot(
    mode: crate::converter::app::ConverterMode,
    snapshots: Vec<DurableJobSnapshot>,
) -> Option<DurableJobSnapshot> {
    snapshots
        .into_iter()
        .filter(|snapshot| daemon_snapshot_matches_mode(mode, snapshot))
        .min_by_key(daemon_snapshot_priority)
}

fn daemon_snapshot_matches_mode(
    mode: crate::converter::app::ConverterMode,
    snapshot: &DurableJobSnapshot,
) -> bool {
    match snapshot.job_class.as_str() {
        "audio-conversion" => mode.is_audio(),
        "video-conversion" => !mode.is_audio(),
        "batch-conversion" => true,
        _ => false,
    }
}

fn daemon_snapshot_priority(snapshot: &DurableJobSnapshot) -> u8 {
    match snapshot.state {
        DurableJobState::Queued | DurableJobState::Running | DurableJobState::Cancelling => 0,
        DurableJobState::Interrupted | DurableJobState::Failed => 1,
        DurableJobState::Completed | DurableJobState::Cancelled => 2,
    }
}

fn daemon_snapshot_is_active(state: DurableJobState) -> bool {
    matches!(
        state,
        DurableJobState::Queued | DurableJobState::Running | DurableJobState::Cancelling
    )
}

fn daemon_snapshot_queue_entry_ids(snapshot: &DurableJobSnapshot) -> Option<Vec<u64>> {
    let details = conversion_details_from_snapshot(snapshot)?;
    let queue_entry_ids = details
        .items
        .iter()
        .map(|conversion_input| conversion_input.daemon_entry_id)
        .collect::<Vec<_>>();
    (!queue_entry_ids.is_empty()).then_some(queue_entry_ids)
}

fn daemon_snapshot_progress_rows(snapshot: &DurableJobSnapshot) -> Option<Vec<(u64, PathBuf)>> {
    let details = conversion_details_from_snapshot(snapshot)?;
    let progress_rows = details
        .items
        .iter()
        .map(|conversion_input| {
            (
                conversion_input.daemon_entry_id,
                conversion_input.input.clone(),
            )
        })
        .collect::<Vec<_>>();
    (!progress_rows.is_empty()).then_some(progress_rows)
}

fn conversion_details_from_snapshot(snapshot: &DurableJobSnapshot) -> Option<ConversionJobDetails> {
    snapshot
        .details
        .as_ref()
        .and_then(|details| serde_json::from_value::<ConversionJobDetails>(details.clone()).ok())
        .filter(|details| details.schema_version == 1)
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

pub(super) fn queue_progress_snapshot(
    runtime_progress_snapshot: big_media_runtime::converter::ffmpeg::progress::FfmpegProgressSnapshot,
) -> FfmpegProgressSnapshot {
    // Single bridge impl lives in `crate::export` so the player embedding and the
    // converter share one runtime→UI FfmpegProgressSnapshot conversion.
    crate::export::ui_progress_snapshot(&runtime_progress_snapshot)
}

#[cfg(test)]
mod daemon_progress_tests {
    use super::*;
    use crate::converter::app::ConverterMode;

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

    #[test]
    fn startup_recovery_prefers_active_daemon_jobs_for_mode() {
        let failed_audio =
            test_snapshot("audio-failed", "audio-conversion", DurableJobState::Failed);
        let running_video = test_snapshot(
            "video-running",
            "video-conversion",
            DurableJobState::Running,
        );
        let completed_video = test_snapshot(
            "video-complete",
            "video-conversion",
            DurableJobState::Completed,
        );

        let selected = select_recoverable_snapshot(
            ConverterMode::Video,
            vec![failed_audio, completed_video, running_video],
        )
        .expect("selected video job");

        assert_eq!(selected.job_id, "video-running");
    }

    #[test]
    fn startup_recovery_reconstructs_daemon_progress_rows() {
        let snapshot = test_snapshot("job-1", "video-conversion", DurableJobState::Interrupted);

        let queue_entry_ids = daemon_snapshot_queue_entry_ids(&snapshot).expect("ids");
        let progress_rows = daemon_snapshot_progress_rows(&snapshot).expect("rows");

        assert_eq!(queue_entry_ids, vec![77]);
        assert_eq!(progress_rows, vec![(77, PathBuf::from("/tmp/input.mp4"))]);
    }

    fn test_snapshot(job_id: &str, job_class: &str, state: DurableJobState) -> DurableJobSnapshot {
        DurableJobSnapshot {
            job_id: job_id.to_owned(),
            job_class: job_class.to_owned(),
            state,
            progress: None,
            status_text: None,
            details: Some(
                serde_json::to_value(ConversionJobDetails {
                    schema_version: 1,
                    items: vec![ConversionEntryDetails {
                        daemon_entry_id: 77,
                        entry_index: 0,
                        input: PathBuf::from("/tmp/input.mp4"),
                        output: Some(PathBuf::from("/tmp/output.mp4")),
                        state: ConversionEntryState::Interrupted,
                        progress: None,
                        command: None,
                        stderr_tail: Vec::new(),
                        status_text: None,
                    }],
                })
                .expect("details"),
            ),
        }
    }
}
