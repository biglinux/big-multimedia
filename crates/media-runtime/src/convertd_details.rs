use super::*;

pub(super) fn conversion_details_value(queue: &ConversionQueue) -> io::Result<serde_json::Value> {
    let details = ConversionJobDetails {
        schema_version: CONVERSION_DETAILS_SCHEMA_VERSION,
        items: queue
            .items()
            .iter()
            .enumerate()
            .map(|(entry_index, conversion_input)| {
                initial_entry_details(entry_index, conversion_input)
            })
            .collect(),
    };
    serde_json::to_value(details).map_err(io::Error::other)
}

pub(super) fn initial_entry_details(
    entry_index: usize,
    conversion_input: &ConversionQueueEntry,
) -> ConversionEntryDetails {
    ConversionEntryDetails {
        daemon_entry_id: conversion_input.id,
        entry_index,
        input: conversion_input.input.clone(),
        output: Some(conversion_input.output.clone()),
        state: ConversionEntryState::Pending,
        progress: None,
        command: None,
        stderr_tail: Vec::new(),
        status_text: None,
    }
}

pub(super) fn read_conversion_details(
    snapshot: &DurableJobSnapshot,
) -> Option<ConversionJobDetails> {
    snapshot
        .details
        .as_ref()
        .and_then(|details| serde_json::from_value(details.clone()).ok())
        .filter(|details: &ConversionJobDetails| {
            details.schema_version == CONVERSION_DETAILS_SCHEMA_VERSION
        })
}

pub(super) fn write_conversion_details(
    snapshot: &mut DurableJobSnapshot,
    details: ConversionJobDetails,
) -> io::Result<()> {
    snapshot.details = Some(serde_json::to_value(details).map_err(io::Error::other)?);
    Ok(())
}

pub(super) fn monitor_conversion_queue(
    registry: Arc<ConversionRegistry>,
    job_id: String,
    receiver: async_channel::Receiver<QueueMessage>,
) {
    let mut completed_entries = HashSet::new();
    let mut failed_items = HashSet::new();
    let mut cancelled_items = HashSet::new();
    while let Ok(message) = receiver.recv_blocking() {
        match message {
            QueueMessage::Started { entry_id, command } => {
                logged(
                    "update_entry_started",
                    update_entry_started(&registry, &job_id, entry_id, command),
                );
            }
            QueueMessage::Progress {
                entry_id,
                progress_snapshot,
            } => {
                logged(
                    "update_entry_progress",
                    update_entry_progress(&registry, &job_id, entry_id, progress_snapshot),
                );
            }
            QueueMessage::Done { entry_id } => {
                completed_entries.insert(entry_id);
                logged(
                    "update_entry_terminal",
                    update_entry_terminal(
                        &registry,
                        &job_id,
                        entry_id,
                        ConversionEntryState::Completed,
                        None,
                    ),
                );
            }
            QueueMessage::Failed { entry_id, error } => {
                failed_items.insert(entry_id);
                logged(
                    "update_entry_terminal",
                    update_entry_terminal(
                        &registry,
                        &job_id,
                        entry_id,
                        ConversionEntryState::Failed,
                        Some(error.clone()),
                    ),
                );
                logged(
                    "set_job_state",
                    set_job_state(
                        &registry,
                        &job_id,
                        DurableJobState::Failed,
                        Some(format!("conversion_input {entry_id} failed: {error}")),
                    ),
                );
            }
            QueueMessage::Cancelled { entry_id } => {
                cancelled_items.insert(entry_id);
                logged(
                    "update_entry_terminal",
                    update_entry_terminal(
                        &registry,
                        &job_id,
                        entry_id,
                        ConversionEntryState::Cancelled,
                        Some("conversion cancelled".to_owned()),
                    ),
                );
                logged(
                    "set_job_state",
                    set_job_state(
                        &registry,
                        &job_id,
                        DurableJobState::Cancelled,
                        Some(format!("conversion_input {entry_id} cancelled")),
                    ),
                );
            }
            QueueMessage::StderrLog { entry_id, lines } => {
                logged(
                    "replace_entry_stderr_tail",
                    replace_entry_stderr_tail(&registry, &job_id, entry_id, lines),
                );
            }
            QueueMessage::StderrLine { entry_id, line } => {
                logged(
                    "append_entry_stderr_line",
                    append_entry_stderr_line(&registry, &job_id, entry_id, line),
                );
            }
            QueueMessage::AllDone => break,
            QueueMessage::GpuFallback { .. } | QueueMessage::Probed { .. } => {}
        }
    }
    let final_state = if failed_items.is_empty() && cancelled_items.is_empty() {
        DurableJobState::Completed
    } else if !cancelled_items.is_empty() {
        DurableJobState::Cancelled
    } else {
        DurableJobState::Failed
    };
    let status_text = match final_state {
        DurableJobState::Completed => Some("conversion completed".to_owned()),
        DurableJobState::Cancelled => Some("conversion cancelled".to_owned()),
        DurableJobState::Failed => Some("conversion failed".to_owned()),
        _ => None,
    };
    logged(
        "finish_job",
        finish_job(&registry, &job_id, final_state, status_text),
    );
}

/// Registry writes are what clients read job state from; a failed one leaves
/// a job looking stuck, so it must at least reach the log.
fn logged<T>(what: &str, result: io::Result<T>) {
    if let Err(error) = result {
        log::warn!("convertd: {what} failed: {error}");
    }
}

pub(super) fn update_entry_started(
    registry: &Arc<ConversionRegistry>,
    job_id: &str,
    entry_id: u64,
    command: String,
) -> io::Result<()> {
    update_conversion_details(registry, job_id, |details| {
        if let Some(conversion_input) = details
            .items
            .iter_mut()
            .find(|conversion_input| conversion_input.daemon_entry_id == entry_id)
        {
            conversion_input.state = ConversionEntryState::Running;
            conversion_input.command = Some(command);
            conversion_input.status_text = Some("ffmpeg running".to_owned());
        }
    })
}

pub(super) fn update_entry_progress(
    registry: &Arc<ConversionRegistry>,
    job_id: &str,
    entry_id: u64,
    progress: FfmpegProgressSnapshot,
) -> io::Result<()> {
    update_conversion_details(registry, job_id, |details| {
        if let Some(conversion_input) = details
            .items
            .iter_mut()
            .find(|conversion_input| conversion_input.daemon_entry_id == entry_id)
        {
            conversion_input.state = ConversionEntryState::Running;
            conversion_input.progress = Some(progress);
            conversion_input.status_text = Some("ffmpeg running".to_owned());
        }
    })
}

pub(super) fn update_entry_terminal(
    registry: &Arc<ConversionRegistry>,
    job_id: &str,
    entry_id: u64,
    state: ConversionEntryState,
    status_text: Option<String>,
) -> io::Result<()> {
    update_conversion_details(registry, job_id, |details| {
        if let Some(conversion_input) = details
            .items
            .iter_mut()
            .find(|conversion_input| conversion_input.daemon_entry_id == entry_id)
        {
            conversion_input.state = state;
            conversion_input.status_text = status_text;
            if matches!(state, ConversionEntryState::Completed) {
                let mut progress = conversion_input.progress.clone().unwrap_or_default();
                progress.percent = 100.0;
                progress.done = true;
                conversion_input.progress = Some(progress);
            }
        }
    })
}

pub(super) fn append_entry_stderr_line(
    registry: &Arc<ConversionRegistry>,
    job_id: &str,
    entry_id: u64,
    line: String,
) -> io::Result<()> {
    update_conversion_details(registry, job_id, |details| {
        if let Some(conversion_input) = details
            .items
            .iter_mut()
            .find(|conversion_input| conversion_input.daemon_entry_id == entry_id)
        {
            conversion_input.stderr_tail.push(line);
            trim_stderr_tail(&mut conversion_input.stderr_tail);
        }
    })
}

pub(super) fn replace_entry_stderr_tail(
    registry: &Arc<ConversionRegistry>,
    job_id: &str,
    entry_id: u64,
    mut lines: Vec<String>,
) -> io::Result<()> {
    trim_stderr_tail(&mut lines);
    update_conversion_details(registry, job_id, |details| {
        if let Some(conversion_input) = details
            .items
            .iter_mut()
            .find(|conversion_input| conversion_input.daemon_entry_id == entry_id)
        {
            conversion_input.stderr_tail = lines;
        }
    })
}

pub(super) fn trim_stderr_tail(lines: &mut Vec<String>) {
    if lines.len() > CONVERSION_ENTRY_STDERR_TAIL_LINES {
        let drop_count = lines.len() - CONVERSION_ENTRY_STDERR_TAIL_LINES;
        lines.drain(0..drop_count);
    }
}

pub(super) fn update_conversion_details(
    registry: &Arc<ConversionRegistry>,
    job_id: &str,
    update: impl FnOnce(&mut ConversionJobDetails),
) -> io::Result<()> {
    {
        let mut jobs = lock_jobs(&registry.jobs)?;
        if let Some(job) = jobs.get_mut(job_id) {
            let mut details =
                read_conversion_details(&job.snapshot).unwrap_or(ConversionJobDetails {
                    schema_version: CONVERSION_DETAILS_SCHEMA_VERSION,
                    items: Vec::new(),
                });
            update(&mut details);
            job.snapshot.progress = Some(aggregate_conversion_progress(&details));
            write_conversion_details(&mut job.snapshot, details)?;
        }
    }
    persist_conversions(registry)
}

pub(super) fn aggregate_conversion_progress(details: &ConversionJobDetails) -> JobProgress {
    let total_units = (details.items.len().max(1) as u64) * 100;
    let completed_units = details
        .items
        .iter()
        .map(|conversion_input| match conversion_input.state {
            ConversionEntryState::Completed => 100_u64,
            ConversionEntryState::Running => {
                conversion_input.progress.as_ref().map_or(0, |progress| {
                    progress.percent.clamp(0.0, 100.0).round() as u64
                })
            }
            ConversionEntryState::Pending
            | ConversionEntryState::Cancelled
            | ConversionEntryState::Interrupted
            | ConversionEntryState::Failed => 0,
        })
        .sum();
    JobProgress::new(completed_units, Some(total_units))
}

#[cfg(test)]
pub(super) fn completed_entry_percent(completed_entries: usize, total_entries: usize) -> f64 {
    (completed_entries as f64 / total_entries.max(1) as f64 * 100.0).clamp(0.0, 100.0)
}
