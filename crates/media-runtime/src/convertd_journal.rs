use super::*;

pub(super) fn sorted_journal_records(
    jobs: &HashMap<String, RuntimeJob>,
) -> Vec<ConversionJournalRecord> {
    let mut entries = jobs
        .values()
        .map(|job| ConversionJournalRecord {
            snapshot: job.snapshot.clone(),
            restart_spec: job.restart_spec.clone(),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.snapshot.job_id.cmp(&right.snapshot.job_id));
    entries
}

pub(super) const fn is_terminal(state: DurableJobState) -> bool {
    matches!(
        state,
        DurableJobState::Completed | DurableJobState::Cancelled | DurableJobState::Failed
    )
}

pub(super) const fn is_active_conversion_state(state: DurableJobState) -> bool {
    matches!(
        state,
        DurableJobState::Queued | DurableJobState::Running | DurableJobState::Cancelling
    )
}

pub(super) fn load_conversion_journal(
    journal_path: &Path,
) -> io::Result<HashMap<String, RuntimeJob>> {
    Ok(big_os_kit::microdaemon::load_durable_job_journal(
        journal_path,
        &[1, CONVERTER_JOURNAL_VERSION],
    )?
    .into_iter()
    .map(|mut record| {
        if !is_terminal(record.snapshot.state) {
            record.snapshot.state = DurableJobState::Interrupted;
            record.snapshot.status_text =
                Some("conversion interrupted by daemon restart; resubmit to retry".to_owned());
            mark_snapshot_items_interrupted(&mut record.snapshot);
        }
        (
            record.snapshot.job_id.clone(),
            RuntimeJob {
                snapshot: record.snapshot,
                restart_spec: record.restart_spec,
                queue: None,
            },
        )
    })
    .collect())
}

pub(super) fn mark_snapshot_items_interrupted(snapshot: &mut DurableJobSnapshot) {
    let Some(mut details) = read_conversion_details(snapshot) else {
        return;
    };
    for conversion_input in &mut details.items {
        if matches!(
            conversion_input.state,
            ConversionEntryState::Pending | ConversionEntryState::Running
        ) {
            conversion_input.state = ConversionEntryState::Interrupted;
            conversion_input.status_text =
                Some("conversion interrupted by daemon restart; resubmit to retry".to_owned());
        }
    }
    let _ = write_conversion_details(snapshot, details);
}

pub(super) fn persist_conversions(registry: &Arc<ConversionRegistry>) -> io::Result<()> {
    let records = {
        let jobs = lock_jobs(&registry.jobs)?;
        sorted_journal_records(&jobs)
    };
    persist_conversion_journal(&registry.journal_path, &records)
}

pub(super) fn persist_conversion_journal(
    journal_path: &Path,
    jobs: &[ConversionJournalRecord],
) -> io::Result<()> {
    big_os_kit::microdaemon::persist_durable_job_journal(
        journal_path,
        CONVERTER_JOURNAL_VERSION,
        jobs,
    )
}

pub(super) fn read_frame(stream: &mut UnixStream) -> io::Result<Option<MicrodaemonFrame>> {
    let mut prefix = [0_u8; 4];
    match stream.read_exact(&mut prefix) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let frame_len = u32::from_be_bytes(prefix) as usize;
    let mut bytes = Vec::with_capacity(4 + frame_len);
    bytes.extend_from_slice(&prefix);
    bytes.resize(4 + frame_len, 0);
    stream.read_exact(&mut bytes[4..])?;
    decode_microdaemon_frame(&bytes)
        .map_err(io::Error::other)
        .map(Some)
}

pub(super) fn write_event(stream: &mut UnixStream, event: MicrodaemonEvent) -> io::Result<()> {
    let frame = MicrodaemonFrame::event(MicrodaemonId::BigConvertd, event);
    let encoded = encode_microdaemon_frame(&frame).map_err(io::Error::other)?;
    stream.write_all(&encoded)
}

pub(super) fn write_error(
    stream: &mut UnixStream,
    job_id: Option<String>,
    code: &str,
    message: &str,
) -> io::Result<()> {
    write_event(
        stream,
        MicrodaemonEvent::Error {
            job_id,
            code: code.to_owned(),
            message: message.to_owned(),
        },
    )
}

pub(super) fn lock_jobs(
    jobs: &Mutex<HashMap<String, RuntimeJob>>,
) -> io::Result<std::sync::MutexGuard<'_, HashMap<String, RuntimeJob>>> {
    jobs.lock()
        .map_err(|_| io::Error::other("conversion registry lock poisoned"))
}

pub(super) fn lock_queue(
    queue: &Arc<Mutex<ConversionQueue>>,
) -> io::Result<std::sync::MutexGuard<'_, ConversionQueue>> {
    queue
        .lock()
        .map_err(|_| io::Error::other("conversion queue lock poisoned"))
}

pub(super) fn reject_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} must be non-empty"))
    } else {
        Ok(())
    }
}

pub(super) fn current_uid() -> libc::uid_t {
    // SAFETY: getuid is a side-effect-free libc query.
    unsafe { libc::getuid() }
}

#[cfg(target_os = "linux")]
pub(super) fn peer_uid(stream: &UnixStream) -> Option<libc::uid_t> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: getsockopt writes at most len bytes into a valid ucred buffer for
    // the provided Unix socket fd.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::addr_of_mut!(credentials).cast(),
            std::ptr::addr_of_mut!(len),
        )
    };
    (rc == 0).then_some(credentials.uid)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn peer_uid(_stream: &UnixStream) -> Option<libc::uid_t> {
    Some(current_uid())
}
