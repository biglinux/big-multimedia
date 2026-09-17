//! Conversion queue: items + profile + GPU set + output dir. Background
//! thread processes pending items sequentially (1 GPU) or round-robin in
//! parallel (>1 GPU). Status transitions reported through `async-channel`
//! and consumed on the UI thread via `drain_worker_messages`.

mod job;
mod processing;
mod receipts;
mod support;
mod types;
mod validation;

#[cfg(test)]
mod tests;

use crate::cast::gpu::{GpuDevice, GpuType, detect_all_gpus};
#[cfg(not(miri))]
use crate::converter::ffmpeg::progress;
use crate::converter::profiles::{self, Profile};
use processing::run_conversion_queue_worker;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use support::dedup_output_path;
#[cfg(not(miri))]
use support::detect_hevc_10bit_to_h264;

pub(crate) use types::ConversionResumeContext;
pub use types::{
    ConversionQueueEntry, ConversionStatus, QueueMessage, SegmentOutputMode, VideoOverrides,
};

pub struct ConversionQueue {
    items: Vec<ConversionQueueEntry>,
    next_id: u64,
    profile: Profile,
    // len==1: sequential; len>1: parallel round-robin.
    gpus: Vec<GpuDevice>,
    output_dir: PathBuf,
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    sender: async_channel::Sender<QueueMessage>,
    receiver: async_channel::Receiver<QueueMessage>,
    resume_context: Option<ConversionResumeContext>,
    /// Hands `add_file` probes to the background worker (see
    /// [`spawn_probe_worker`]); results come back as [`QueueMessage::Probed`].
    probe_tx: std::sync::mpsc::SyncSender<ProbeRequest>,
    pub delete_source: bool,
    pub audio_only_mode: bool,
}

impl Default for ConversionQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversionQueue {
    #[must_use]
    pub fn new() -> Self {
        let (sender, receiver) = async_channel::bounded(256);
        let probe_tx = spawn_probe_worker(sender.clone());
        let gpus = detect_all_gpus();
        let default_gpu = gpus.first().cloned().unwrap_or(GpuDevice {
            gpu_type: GpuType::Cpu,
            name: "CPU".into(),
            device_path: String::new(),
        });

        Self {
            items: Vec::new(),
            next_id: 1,
            probe_tx,
            profile: Profile {
                profile: profiles::ProfileMeta::default(),
                video: profiles::VideoConfig::default(),
                audio: profiles::AudioConfig::default(),
                subtitles: profiles::SubtitleConfig::default(),
                output: profiles::OutputConfig::default(),
            },
            gpus: vec![default_gpu],
            output_dir: big_os_kit::xdg_dirs::video_dir().unwrap_or_else(|| PathBuf::from(".")),
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            sender,
            receiver,
            resume_context: Some(ConversionResumeContext {
                job_key: "verified-receipt-v1".into(),
            }),
            delete_source: false,
            audio_only_mode: false,
        }
    }

    pub fn set_audio_only_mode(&mut self, enabled: bool) {
        self.audio_only_mode = enabled;
        if enabled {
            self.output_dir =
                big_os_kit::xdg_dirs::audio_dir().unwrap_or_else(|| PathBuf::from("."));
        }
    }

    /// Reuse only outputs whose source, plan, digest and media validation match
    /// their durable receipt. This is not mid-bitstream continuation.
    pub fn set_verified_resume(&mut self, enabled: bool) {
        self.resume_context = enabled.then(|| ConversionResumeContext {
            job_key: "verified-receipt-v1".into(),
        });
    }

    #[must_use]
    pub fn receiver(&self) -> &async_channel::Receiver<QueueMessage> {
        &self.receiver
    }

    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
    }

    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    pub fn set_gpus(&mut self, gpus: Vec<GpuDevice>) {
        self.gpus = if gpus.is_empty() {
            vec![GpuDevice {
                gpu_type: GpuType::Cpu,
                name: "CPU".into(),
                device_path: String::new(),
            }]
        } else {
            gpus
        };
    }

    pub fn set_gpu(&mut self, gpu: GpuDevice) {
        self.gpus = vec![gpu];
    }

    #[must_use]
    pub fn gpu(&self) -> &GpuDevice {
        &self.gpus[0]
    }

    #[must_use]
    pub fn gpu_count(&self) -> usize {
        self.gpus.len()
    }

    pub fn set_output_dir(&mut self, dir: PathBuf) {
        self.output_dir = dir;
    }

    #[must_use]
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    #[must_use]
    pub fn add_file(&mut self, input: PathBuf) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let output_name = if self.audio_only_mode {
            profiles::resolve_audio_output_name(&self.profile, &input)
        } else {
            profiles::resolve_output_name(&self.profile, &input)
        };
        let requested = self.output_dir.join(output_name);
        let output = if self.resume_context.is_some() {
            // The same plan must map to the same output across sessions. Avoid
            // probing existence in GTK; the worker verifies receipts and refuses
            // unknown outputs. Preserve the input even when names coincide.
            if requested == input {
                let mut stem = requested.file_stem().unwrap_or_default().to_os_string();
                stem.push("_converted");
                if let Some(extension) = requested.extension() {
                    stem.push(".");
                    stem.push(extension);
                }
                requested.with_file_name(stem)
            } else {
                requested
            }
        } else {
            dedup_output_path(&requested, &input)
        };
        // Probe OFF the main thread: duration + audio channels + HEVC-10bit
        // detection are three ffprobe runs PER FILE — synchronous here they
        // froze every window of an embedding host while adding a batch. The
        // worker answers with QueueMessage::Probed; until then the queue_entry
        // carries safe defaults (no % until duration arrives, software
        // filters kept — the GPU-fallback path already covers that).
        #[cfg(not(miri))]
        let probe_request = ProbeRequest {
            entry_id: id,
            input: input.clone(),
            target_codec: self.profile.video.codec.clone(),
        };
        #[cfg(miri)]
        let probe_request = ProbeRequest;
        let _ = self.probe_tx.try_send(probe_request);

        let queue_entry = ConversionQueueEntry {
            id,
            input,
            output,
            status: ConversionStatus::Pending,
            duration_secs: 0.0,
            profile_name: self.profile.profile.name.clone(),
            segments: Vec::new(),
            segment_mode: SegmentOutputMode::Join,
            subtitle: crate::converter::subtitle::SubtitleInput::EmbeddedCopy,
            audio_track: None,
            video_track: None,
            crop: None,
            video_overrides: None,
            input_audio_channels: 0,
            skip_video_filters: false,
            probed: false,
        };
        self.items.push(queue_entry);
        id
    }

    pub fn remove_entry(&mut self, id: u64) -> bool {
        if let Some(pos) = self
            .items
            .iter()
            .position(|queue_entry| queue_entry.id == id)
        {
            self.items.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn clear_pending(&mut self) {
        self.items
            .retain(|queue_entry| !matches!(queue_entry.status, ConversionStatus::Pending));
    }

    pub fn clear_all(&mut self) {
        self.items.clear();
    }

    // Retry path after cancel: requeue everything that didn't finish.
    pub fn reset_non_done(&mut self) {
        for queue_entry in &mut self.items {
            if matches!(
                queue_entry.status,
                ConversionStatus::Converting
                    | ConversionStatus::Cancelled
                    | ConversionStatus::Failed(_)
            ) {
                queue_entry.status = ConversionStatus::Pending;
            }
        }
    }

    #[must_use]
    pub fn items(&self) -> &[ConversionQueueEntry] {
        &self.items
    }

    #[must_use]
    pub fn entry_mut(&mut self, id: u64) -> Option<&mut ConversionQueueEntry> {
        self.items
            .iter_mut()
            .find(|queue_entry| queue_entry.id == id)
    }

    pub fn set_pending_segment_mode(&mut self, mode: SegmentOutputMode) {
        for queue_entry in &mut self.items {
            if queue_entry.status == ConversionStatus::Pending {
                queue_entry.segment_mode = mode;
            }
        }
    }

    pub fn sort_pending_segments_chronological(&mut self) {
        for queue_entry in &mut self.items {
            if queue_entry.status != ConversionStatus::Pending || queue_entry.segments.len() < 2 {
                continue;
            }
            queue_entry.segments.sort_by(|a, b| {
                let ak = a.start.unwrap_or(0.0);
                let bk = b.start.unwrap_or(0.0);
                ak.partial_cmp(&bk).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }

    pub fn clear_pending_segments(&mut self) {
        for queue_entry in &mut self.items {
            if queue_entry.status == ConversionStatus::Pending {
                queue_entry.segments.clear();
            }
        }
    }

    pub fn reorder(&mut self, order: &[u64]) {
        let mut sorted = Vec::with_capacity(self.items.len());
        for &id in order {
            if let Some(pos) = self
                .items
                .iter()
                .position(|queue_entry| queue_entry.id == id)
            {
                sorted.push(self.items.remove(pos));
            }
        }
        sorted.append(&mut self.items);
        self.items = sorted;
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn start(&mut self) {
        if self.sender.is_closed()
            || self
                .running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            // Repeated clicks must not reset a live worker's cancellation flag
            // or start two writers for the same output plan.
            log::warn!("export queue is already running or closed");
            return;
        }
        let run = QueueRun {
            running: self.running.clone(),
            sender: self.sender.clone(),
            cancel: self.cancel.clone(),
        };
        self.cancel.store(false, Ordering::Release);

        let items: Vec<ConversionQueueEntry> = self
            .items
            .iter()
            .filter(|queue_entry| matches!(queue_entry.status, ConversionStatus::Pending))
            .cloned()
            .collect();

        let profile = self.profile.clone();
        let gpus = self.gpus.clone();
        let cancel = self.cancel.clone();
        let sender = self.sender.clone();
        let resume_context = self.resume_context.clone();
        let delete_source = self.delete_source;
        let audio_only_mode = self.audio_only_mode;

        let drain = big_os_kit::background::track_operation();
        let spawned = std::thread::Builder::new()
            .name("converter-queue".into())
            .spawn(move || {
                let _drain = drain;
                let _run = run;
                if gpus.len() > 1 && items.len() > 1 {
                    let mut gpu_batches: Vec<Vec<ConversionQueueEntry>> =
                        vec![Vec::new(); gpus.len()];
                    for (index, queue_entry) in items.into_iter().enumerate() {
                        gpu_batches[index % gpus.len()].push(queue_entry);
                    }
                    log::info!(
                        "Parallel conversion: {} GPUs, batches: {:?}",
                        gpus.len(),
                        gpu_batches.iter().map(Vec::len).collect::<Vec<_>>()
                    );
                    std::thread::scope(|scope| {
                        for (idx, batch) in gpu_batches.into_iter().enumerate() {
                            if batch.is_empty() {
                                continue;
                            }
                            let gpu = &gpus[idx];
                            let profile = &profile;
                            let cancel = &cancel;
                            let sender = &sender;
                            let resume_context = resume_context.clone();
                            scope.spawn(move || {
                                run_conversion_queue_worker(
                                    batch,
                                    profile,
                                    gpu,
                                    cancel,
                                    sender,
                                    resume_context,
                                    delete_source,
                                    audio_only_mode,
                                );
                            });
                        }
                    });
                } else {
                    let gpu = gpus.first().cloned().unwrap_or(GpuDevice {
                        gpu_type: GpuType::Cpu,
                        name: "CPU".into(),
                        device_path: String::new(),
                    });
                    run_conversion_queue_worker(
                        items,
                        &profile,
                        &gpu,
                        &cancel,
                        &sender,
                        resume_context,
                        delete_source,
                        audio_only_mode,
                    );
                }
                let _ = sender.send_blocking(QueueMessage::AllDone);
            });
        if spawned.is_err() {
            for entry in &mut self.items {
                if entry.status == ConversionStatus::Pending {
                    entry.status = ConversionStatus::Failed("Could not start export worker".into());
                    let _ = self.sender.try_send(QueueMessage::Failed {
                        entry_id: entry.id,
                        error: "Could not start export worker".into(),
                    });
                }
            }
            // Never leave a blocking consumer waiting for a worker that did not
            // start, even when its message buffer was already occupied by probes.
            self.sender.close();
            return;
        }

        for queue_entry in &mut self.items {
            if matches!(queue_entry.status, ConversionStatus::Pending) {
                queue_entry.status = ConversionStatus::Converting;
            }
        }
    }

    // UI-thread drain. Pulls Done/Failed/Cancelled into queue_entry.status.
    pub fn drain_worker_messages(&mut self) {
        while let Ok(worker_message) = self.receiver.try_recv() {
            match worker_message {
                QueueMessage::Done { entry_id } => {
                    if let Some(queue_entry) = self
                        .items
                        .iter_mut()
                        .find(|queue_entry| queue_entry.id == entry_id)
                    {
                        queue_entry.status = ConversionStatus::Done;
                    }
                }
                QueueMessage::Failed { entry_id, error } => {
                    if let Some(queue_entry) = self
                        .items
                        .iter_mut()
                        .find(|queue_entry| queue_entry.id == entry_id)
                    {
                        queue_entry.status = ConversionStatus::Failed(error);
                    }
                }
                QueueMessage::Cancelled { entry_id } => {
                    if let Some(queue_entry) = self
                        .items
                        .iter_mut()
                        .find(|queue_entry| queue_entry.id == entry_id)
                    {
                        queue_entry.status = ConversionStatus::Cancelled;
                    }
                }
                QueueMessage::Probed {
                    entry_id,
                    duration_secs,
                    input_audio_channels,
                    skip_video_filters,
                } => {
                    if let Some(queue_entry) = self
                        .items
                        .iter_mut()
                        .find(|queue_entry| queue_entry.id == entry_id)
                    {
                        queue_entry.duration_secs = duration_secs;
                        queue_entry.input_audio_channels = input_audio_channels;
                        queue_entry.skip_video_filters = skip_video_filters;
                        queue_entry.probed = true;
                    }
                }
                _ => {}
            }
        }
    }

    #[must_use]
    pub fn count(&self, status: &ConversionStatus) -> usize {
        self.items
            .iter()
            .filter(|queue_entry| &queue_entry.status == status)
            .count()
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.items.len()
    }
}

/// One queued background-probe request (see [`spawn_probe_worker`]).
#[cfg(miri)]
struct ProbeRequest;

/// One queued background-probe request (see [`spawn_probe_worker`]).
#[cfg(not(miri))]
struct ProbeRequest {
    entry_id: u64,
    input: PathBuf,
    target_codec: String,
}

/// Single sequential worker for `add_file` probes: one ffprobe at a time
/// keeps an old machine from a process storm, while the main loop stays
/// untouched. Results return through the queue's message channel.
#[cfg(miri)]
fn spawn_probe_worker(
    _sender: async_channel::Sender<QueueMessage>,
) -> std::sync::mpsc::SyncSender<ProbeRequest> {
    let (tx, _rx) = std::sync::mpsc::sync_channel::<ProbeRequest>(64);
    tx
}

/// Single sequential worker for `add_file` probes: one ffprobe at a time
/// keeps an old machine from a process storm, while the main loop stays
/// untouched. Results return through the queue's message channel.
#[cfg(not(miri))]
fn spawn_probe_worker(
    sender: async_channel::Sender<QueueMessage>,
) -> std::sync::mpsc::SyncSender<ProbeRequest> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<ProbeRequest>(64);
    let spawned = std::thread::Builder::new()
        .name("converter-probe".into())
        .spawn(move || {
            while let Ok(request) = rx.recv() {
                if sender.is_closed() {
                    break;
                }
                let duration_secs = progress::probe_duration(&request.input);
                let input_audio_channels = progress::probe_audio_channels(&request.input);
                let skip_video_filters =
                    detect_hevc_10bit_to_h264(&request.input, &request.target_codec);
                let _ = sender.send_blocking(QueueMessage::Probed {
                    entry_id: request.entry_id,
                    duration_secs,
                    input_audio_channels,
                    skip_video_filters,
                });
            }
        });
    if let Err(error) = spawned {
        log::error!("converter probe worker failed to spawn: {error}");
    }
    tx
}

impl Drop for ConversionQueue {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        // Wake a producer stalled on a full progress channel. Its cancellation
        // path owns and cleans only its private staging, not published outputs.
        self.sender.close();
    }
}

// Cleared on completion, spawn failure or unwind; never reset cancellation of
// the previous run before it has actually terminated.
struct QueueRun {
    running: Arc<AtomicBool>,
    sender: async_channel::Sender<QueueMessage>,
    cancel: Arc<AtomicBool>,
}
impl Drop for QueueRun {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.cancel.store(true, Ordering::Release);
            self.sender.close();
        }
        self.running.store(false, Ordering::Release);
    }
}
