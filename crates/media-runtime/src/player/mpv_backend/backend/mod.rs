//! `MpvBackend` — mpv ownership, background event loop, lifecycle.
//! Per-mode initialization options live in `init_options`. Split from
//! the original monolithic `backend.rs`; public API is unchanged.

mod client;
mod init_options;

pub use client::MpvError;
use client::{MpvClient, MpvEvent, MpvPropertyValue};
use log::{debug, info};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::{PlayerEvent, TrackMetadata};
use init_options::{
    apply_audio_only_options, apply_audio_output, apply_custom_options, apply_input_ipc,
    apply_playback_options, apply_video_options, apply_watch_later, parse_custom_options,
};

/// Accumulated state for the mpv event_loop: last-seen property values +
/// a small queue of events that were produced inside the event-drain phase
/// but must be emitted after properties are polled (so UI sees a coherent
/// order on FileLoaded).
#[derive(Default)]
struct PollState {
    last_pos: f64,
    last_dur: f64,
    last_paused: Option<bool>,
    last_buffering: Option<bool>,
    last_eof: bool,
    last_title: String,
    last_volume: Option<f64>,
    last_mute: Option<bool>,
    pending: Vec<PlayerEvent>,
}

impl PollState {
    fn reset_on_load(&mut self) {
        self.last_pos = -1.0;
        self.last_dur = -1.0;
        self.last_eof = false;
    }
}

/// Owned-data intermediate so we can drop mpv's event (which borrows from
/// the mpv event before calling back into mpv for follow-up properties.
enum MappedEvent {
    FileLoaded,
    EndFile(u32),
    Shutdown,
    Position(f64),
    Duration(f64),
    Paused(bool),
    Volume(f64),
    CommandReply { id: u64, error: i32 },
}

pub struct MpvBackend {
    /// Locked mpv client. Drop uses mpv_terminate_destroy instead of plain
    /// mpv_destroy so libmpv internal threads are joined before teardown.
    pub(in crate::player::mpv_backend) mpv: Arc<Mutex<MpvClient>>,
    /// Raw mpv handle pointer, cached for lock-free async commands.
    /// mpv_command_async is internally thread-safe, so we don't need the mutex.
    /// The pointer is valid for the lifetime of the mpv instance (held by `mpv`).
    pub(in crate::player::mpv_backend) raw_mpv_handle: *mut mpv_sys::mpv_handle,
    pub(in crate::player::mpv_backend) running: Arc<AtomicBool>,
    /// Set to true by `load_file()`, cleared by the event loop on `FileLoaded`.
    /// Suppresses the eof-reached poll during file transitions to prevent
    /// spurious EndFile(0) events that cause unwanted queue advance.
    pub(in crate::player::mpv_backend) loading: Arc<AtomicBool>,
    pub(in crate::player::mpv_backend) audio_only: bool,
    pub(in crate::player::mpv_backend) screenshots: Arc<super::screenshots::Screenshots>,
    event_thread: Option<std::thread::JoinHandle<()>>,
    event_sender: async_channel::Sender<PlayerEvent>,
    /// Tracks whether audio filters are currently set, so we can skip
    /// the expensive ao-reload when clearing an already-empty chain.
    pub(in crate::player::mpv_backend) has_af: AtomicBool,
    pub(in crate::player::mpv_backend) stream_volume: super::pactl_volume::StreamVolume,
}

// SAFETY: raw_mpv_handle points to an mpv_handle that lives as long as the Arc<Mutex<MpvClient>>.
// mpv_command_async is documented as thread-safe on the same handle
unsafe impl Send for MpvBackend {}
unsafe impl Sync for MpvBackend {}

impl MpvBackend {
    /// Create a new MpvBackend using vo=libmpv for render API integration.
    /// `custom_options` are applied during initialization (e.g. "cache=yes,demuxer-max-bytes=512MiB")
    /// `audio_output` sets the audio driver (e.g. "pipewire", "pulse", "" for auto)
    pub fn new(
        sender: async_channel::Sender<PlayerEvent>,
        custom_options: &str,
        audio_output: &str,
    ) -> Result<Self, MpvError> {
        let custom_opts = parse_custom_options(custom_options);

        let mpv = MpvClient::with_initializer(|init| {
            apply_video_options(init)?;
            apply_audio_output(init, audio_output);
            apply_playback_options(init)?;
            apply_watch_later(init, crate::config::video::WATCH_LATER_RELATIVE)?;
            apply_input_ipc(init, crate::config::video::MPV_IPC_SOCKET);
            apply_custom_options(init, &custom_opts);
            Ok(())
        })?;

        Ok(Self::finish_mpv_init(
            mpv,
            sender,
            "mpv-events",
            "MpvBackend initialized", false,
        ))
    }

    /// Create a new MpvBackend in audio-only mode (no video output).
    pub fn new_audio_only(sender: async_channel::Sender<PlayerEvent>) -> Result<Self, MpvError> {
        Self::new_audio_only_with_output(sender, "")
    }

    /// Create an audio engine using the saved output driver (empty means auto).
    pub fn new_audio_only_with_output(
        sender: async_channel::Sender<PlayerEvent>,
        audio_output: &str,
    ) -> Result<Self, MpvError> {
        let mpv = MpvClient::with_initializer(|init| {
            apply_audio_only_options(init)?;
            apply_audio_output(init, audio_output);
            apply_watch_later(init, crate::config::audio::WATCH_LATER_RELATIVE)?;
            Ok(())
        })?;

        Ok(Self::finish_mpv_init(
            mpv,
            sender,
            "mpv-audio-events",
            "MpvBackend initialized (audio-only)", true,
        ))
    }

    fn finish_mpv_init(
        mpv: MpvClient,
        sender: async_channel::Sender<PlayerEvent>,
        thread_name: &str,
        init_log: &str,
        audio_only: bool,
    ) -> Self {
        let mpv = Arc::new(Mutex::new(mpv));
        let raw_mpv_handle = mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .raw_handle();
        let running = Arc::new(AtomicBool::new(true));
        let loading = Arc::new(AtomicBool::new(false));
        let run_flag = running.clone();
        let load_flag = loading.clone();
        let mpv_clone = mpv.clone();
        let audio_client_name = init_options::audio_client_name();
        let stream_volume = super::pactl_volume::StreamVolume::new(audio_client_name);
        let volume_state = stream_volume.shared();

        let screenshots = Arc::new(super::screenshots::Screenshots::new());
        let screenshots_for_thread = screenshots.clone();
        let event_sender = sender.clone();
        let event_thread = std::thread::Builder::new()
            .name(thread_name.into())
            .spawn(move || {
                Self::event_loop(
                    mpv_clone,
                    run_flag,
                    load_flag,
                    sender,
                    volume_state,
                    screenshots_for_thread,
                );
            })
            .expect("Failed to spawn mpv event thread");

        info!("{init_log}");
        Self {
            mpv,
            raw_mpv_handle,
            running,
            loading,
            audio_only,
            screenshots,
            event_thread: Some(event_thread),
            event_sender,
            has_af: AtomicBool::new(false),
            stream_volume,
        }
    }

    fn event_loop(
        mpv: Arc<Mutex<MpvClient>>,
        running: Arc<AtomicBool>,
        loading: Arc<AtomicBool>,
        sender: async_channel::Sender<PlayerEvent>,
        volume_state: super::pactl_volume::Shared,
        screenshots: Arc<super::screenshots::Screenshots>,
    ) {
        let mut poll = PollState::default();
        let mut outgoing = Vec::with_capacity(80);
        let mut pending = std::collections::VecDeque::with_capacity(96);
        let handle = mpv.lock().unwrap_or_else(std::sync::PoisonError::into_inner).raw_handle();

        while running.load(Ordering::Relaxed) && !sender.is_closed() {
            for id in screenshots.withdraw_expired() {
                // The registry still owns the staging file until terminal receipt.
                unsafe { mpv_sys::mpv_abort_async_command(handle, id); }
                pending.push_back(PlayerEvent::ScreenshotTimedOut { id });
            }
            while let Some(event) = pending.pop_front() {
                match sender.try_send(event) {
                    Ok(()) => {},
                    Err(async_channel::TrySendError::Full(event)) => { pending.push_front(event); break; },
                    Err(async_channel::TrySendError::Closed(_)) => return,
                }
            }
            if !pending.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            {
                let mut guard = mpv
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Self::drain_event_queue(&mut guard, &loading, &mut poll, &mut outgoing);
                Self::poll_properties(&guard, &loading, &mut poll, &mut outgoing);
            }
            // Never wait for the UI while holding the mpv mutex: a UI handler
            // may need that same mutex before it can drain the bounded channel.
            for event in outgoing.drain(..) {
                let event = match event {
                    PlayerEvent::AsyncCommandReply { id, error } => {
                        let Some(result) = screenshots.complete(id, error) else { continue; };
                        PlayerEvent::ScreenshotFinished { id, result }
                    }
                    other => other,
                };
                match &event {
                    PlayerEvent::FileLoaded(_) => volume_state.set_active(true),
                    PlayerEvent::EndFile(_) | PlayerEvent::Shutdown => volume_state.set_active(false),
                    _ => {}
                }
                pending.push_back(event);
            }
            Self::poll_stream_volume(&volume_state, &sender, &mut poll);
            // Position feedback stays smooth during playback, but a paused
            // player does not need to poll every property twenty times/second.
            let interval = if poll.last_paused == Some(true) { 200 } else { 50 };
            std::thread::sleep(std::time::Duration::from_millis(interval));
        }
        debug!("MPV event loop exited");
    }

    /// Mirror the worker snapshot. Sound-server I/O never blocks mpv events.
    fn poll_stream_volume(
        volume_state: &super::pactl_volume::Shared,
        sender: &async_channel::Sender<PlayerEvent>,
        poll: &mut PollState,
    ) {
        // Reading the worker snapshot cannot spawn or wait on a process.
        if let Some((volume, mute)) = volume_state.snapshot() {
            if poll
                .last_volume
                .is_none_or(|last| (last - volume).abs() > 0.5)
            {
                poll.last_volume = Some(volume);
                let _ = sender.try_send(PlayerEvent::VolumeChanged(volume));
            }
            if poll.last_mute != Some(mute) {
                poll.last_mute = Some(mute);
                let _ = sender.try_send(PlayerEvent::MuteChanged(mute));
            }
        }
    }

    /// Collect owned events under the mpv lock, without sending or waiting.
    /// A bounded batch prevents an event burst monopolising the client mutex.
    fn drain_event_queue(
        mpv_guard: &mut MpvClient,
        loading: &Arc<AtomicBool>,
        poll: &mut PollState,
        outgoing: &mut Vec<PlayerEvent>,
    ) {
        for _ in 0..64 {
            let Some(event) = Self::next_event(mpv_guard, loading, poll) else {
                break;
            };
            let shutdown = matches!(&event, PlayerEvent::Shutdown);
            outgoing.push(event);
            if shutdown {
                break;
            }
        }
    }

    /// Pull one event from mpv. Returns `None` when the queue is empty
    /// (drain done) or the event wasn't interesting. Side-effects: updates
    /// `loading`/`poll` flags for lifecycle events.
    fn next_event(
        mpv_guard: &mut MpvClient,
        loading: &Arc<AtomicBool>,
        poll: &mut PollState,
    ) -> Option<PlayerEvent> {
        loop {
            // Resolve and own the event data before the next wait_event call.
            let mapped = match mpv_guard.wait_event(0.0)? {
                Ok(MpvEvent::FileLoaded) => MappedEvent::FileLoaded,
                Ok(MpvEvent::EndFile(reason)) => MappedEvent::EndFile(reason),
                Ok(MpvEvent::Shutdown) => MappedEvent::Shutdown,
                Ok(MpvEvent::CommandReply { id: 0, .. }) => continue,
                Ok(MpvEvent::CommandReply { id, error }) => MappedEvent::CommandReply { id, error },
                Ok(MpvEvent::PropertyChange { name, change }) => match (name.as_str(), change) {
                    ("time-pos", MpvPropertyValue::Double(v)) => MappedEvent::Position(v),
                    ("duration", MpvPropertyValue::Double(v)) => MappedEvent::Duration(v),
                    ("pause", MpvPropertyValue::Flag(v)) => MappedEvent::Paused(v),
                    ("volume", MpvPropertyValue::Double(v)) => MappedEvent::Volume(v),
                    _ => continue,
                },
                _ => continue,
            };
            return Some(Self::finalize_event(mapped, mpv_guard, loading, poll));
        }
    }

    fn finalize_event(
        mapped: MappedEvent,
        mpv_guard: &MpvClient,
        loading: &Arc<AtomicBool>,
        poll: &mut PollState,
    ) -> PlayerEvent {
        match mapped {
            MappedEvent::FileLoaded => {
                loading.store(false, Ordering::Relaxed);
                poll.reset_on_load();
                let title = mpv_guard
                    .get_property::<String>("media-title")
                    .unwrap_or_default();
                if let Ok(tracks) = Self::get_track_list_locked(mpv_guard) {
                    poll.pending.push(PlayerEvent::TracksChanged(tracks));
                }
                if let Ok(chapters) = Self::get_chapters_locked(mpv_guard) {
                    poll.pending.push(PlayerEvent::ChaptersChanged(chapters));
                }
                PlayerEvent::FileLoaded(title)
            }
            MappedEvent::EndFile(reason) => {
                poll.last_eof = true;
                log::debug!("[MPV-EVENT] EndFile reason={reason} (from mpv event queue)");
                PlayerEvent::EndFile(reason)
            }
            MappedEvent::Shutdown => PlayerEvent::Shutdown,
            MappedEvent::Position(v) => PlayerEvent::PositionChanged(v),
            MappedEvent::Duration(v) => PlayerEvent::DurationChanged(v),
            MappedEvent::Paused(v) => PlayerEvent::PauseChanged(v),
            MappedEvent::Volume(v) => PlayerEvent::VolumeChanged(v),
            MappedEvent::CommandReply { id, error } => PlayerEvent::AsyncCommandReply { id, error },
        }
    }

    /// Poll time/duration/pause/eof/title/buffering and dispatch diff-based
    /// `PlayerEvent`s. Runs at ~20 Hz from the main event_loop.
    fn poll_properties(
        mpv_guard: &MpvClient,
        loading: &Arc<AtomicBool>,
        poll: &mut PollState,
        outgoing: &mut Vec<PlayerEvent>,
    ) {
        outgoing.append(&mut poll.pending);
        if let Ok(pos) = mpv_guard.get_property::<f64>("time-pos")
            && (pos - poll.last_pos).abs() > 0.05
        {
            poll.last_pos = pos;
            outgoing.push(PlayerEvent::PositionChanged(pos));
        }
        if let Ok(dur) = mpv_guard.get_property::<f64>("duration")
            && (dur - poll.last_dur).abs() > 0.1
        {
            poll.last_dur = dur;
            outgoing.push(PlayerEvent::DurationChanged(dur));
        }
        if let Ok(paused) = Self::get_bool_property(mpv_guard, "pause")
            && poll.last_paused != Some(paused)
        {
            poll.last_paused = Some(paused);
            outgoing.push(PlayerEvent::PauseChanged(paused));
        }
        if loading.load(Ordering::Relaxed) {
            log::debug!("[MPV-POLL] skipping eof-reached check (loading=true)");
        } else if let Ok(eof) = Self::get_bool_property(mpv_guard, "eof-reached") {
            if eof && !poll.last_eof {
                log::debug!("[MPV-POLL] eof-reached=true, sending synthetic EndFile(0)");
                outgoing.push(PlayerEvent::EndFile(0));
            }
            poll.last_eof = eof;
        }
        if let Ok(title) = mpv_guard.get_property::<String>("media-title")
            && !title.is_empty()
            && title != poll.last_title
        {
            poll.last_title.clone_from(&title);
            let mut meta = HashMap::new();
            meta.insert("title".to_string(), title);
            if let Ok(artist) = mpv_guard.get_property::<String>("metadata/by-key/artist") {
                meta.insert("artist".to_string(), artist);
            }
            outgoing.push(PlayerEvent::MetadataChanged(meta));
        }
        if let Ok(buffering) = Self::get_bool_property(mpv_guard, "paused-for-cache")
            && poll.last_buffering != Some(buffering)
        {
            poll.last_buffering = Some(buffering);
            outgoing.push(PlayerEvent::Buffering(buffering));
        }
    }

    fn get_track_list_locked(mpv: &MpvClient) -> Result<Vec<TrackMetadata>, MpvError> {
        let count = mpv.get_property::<i64>("track-list/count")?;
        let mut tracks = Vec::new();
        for i in 0..count {
            let prefix = format!("track-list/{i}");
            let id = mpv
                .get_property::<i64>(&format!("{prefix}/id"))
                .unwrap_or(0);
            let track_type = mpv
                .get_property::<String>(&format!("{prefix}/type"))
                .unwrap_or_default();
            let title = mpv
                .get_property::<String>(&format!("{prefix}/title"))
                .unwrap_or_default();
            let lang = mpv
                .get_property::<String>(&format!("{prefix}/lang"))
                .unwrap_or_default();
            let selected = mpv
                .get_property::<String>(&format!("{prefix}/selected"))
                .is_ok_and(|value| value == "yes");
            let codec = mpv
                .get_property::<String>(&format!("{prefix}/codec"))
                .unwrap_or_default();
            let external = mpv
                .get_property::<String>(&format!("{prefix}/external"))
                .is_ok_and(|value| value == "yes");
            tracks.push(TrackMetadata {
                id,
                track_type,
                title,
                lang,
                selected,
                codec,
                external,
            });
        }
        Ok(tracks)
    }

    pub fn get_track_list(&self) -> Vec<TrackMetadata> {
        self.with_mpv(|mpv| Self::get_track_list_locked(mpv).unwrap_or_default())
    }

    fn get_chapters_locked(mpv: &MpvClient) -> Result<Vec<(f64, String)>, MpvError> {
        let count = mpv.get_property::<i64>("chapter-list/count")?;
        let mut chapters = Vec::new();
        for i in 0..count {
            let time = mpv
                .get_property::<f64>(&format!("chapter-list/{i}/time"))
                .unwrap_or(0.0);
            let raw_title = mpv
                .get_property::<String>(&format!("chapter-list/{i}/title"))
                .unwrap_or_default();
            let title = if raw_title.is_empty()
                || raw_title.eq_ignore_ascii_case("(unnamed)")
                || raw_title.eq_ignore_ascii_case("unnamed")
            {
                format!("Chapter {}", i + 1)
            } else {
                raw_title
            };
            chapters.push((time, title));
        }
        Ok(chapters)
    }

    pub(in crate::player::mpv_backend) fn with_mpv<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&MpvClient) -> R,
    {
        let guard = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&guard)
    }

    /// Safe bool property wrapper. `MPV_FORMAT_FLAG` is C `int` storage.
    pub(in crate::player::mpv_backend) fn get_bool_property(
        mpv: &MpvClient,
        name: &str,
    ) -> Result<bool, MpvError> {
        mpv.get_property::<bool>(name)
    }
}

impl Drop for MpvBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // A full channel may have the worker waiting for a GUI that is itself
        // inside Drop. Closing wakes send_blocking before joining the worker.
        self.event_sender.close();
        if let Some(handle) = self.event_thread.take() {
            let _ = handle.join();
        }

        self.mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .terminate_destroy();
        self.screenshots.clear_after_termination();

        info!("MpvBackend dropped (mpv_terminate_destroy)");
    }
}
