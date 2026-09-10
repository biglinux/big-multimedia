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
    last_volume_poll: Option<std::time::Instant>,
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
    event_thread: Option<std::thread::JoinHandle<()>>,
    /// Tracks whether audio filters are currently set, so we can skip
    /// the expensive ao-reload when clearing an already-empty chain.
    pub(in crate::player::mpv_backend) has_af: AtomicBool,
    /// `audio-client-name` mpv reports to the sound server (== `application.name`
    /// on our sink-input). Used to locate our own stream for volume control.
    pub(in crate::player::mpv_backend) audio_client_name: String,
    /// Cached sink-input index for our stream (mpv recreates it per file, so this
    /// is re-resolved when a `pactl` call finds it stale). Shared with the event
    /// loop, which polls the stream to mirror external mixer changes back to the
    /// UI. See `pactl_volume`.
    pub(in crate::player::mpv_backend) pa_sink_input: Arc<Mutex<Option<u32>>>,
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
            "MpvBackend initialized",
        ))
    }

    /// Create a new MpvBackend in audio-only mode (no video output).
    pub fn new_audio_only(sender: async_channel::Sender<PlayerEvent>) -> Result<Self, MpvError> {
        let mpv = MpvClient::with_initializer(|init| {
            apply_audio_only_options(init)?;
            apply_watch_later(init, crate::config::audio::WATCH_LATER_RELATIVE)?;
            Ok(())
        })?;

        Ok(Self::finish_mpv_init(
            mpv,
            sender,
            "mpv-audio-events",
            "MpvBackend initialized (audio-only)",
        ))
    }

    fn finish_mpv_init(
        mpv: MpvClient,
        sender: async_channel::Sender<PlayerEvent>,
        thread_name: &str,
        init_log: &str,
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
        let pa_sink_input = Arc::new(Mutex::new(None));
        let poll_client = audio_client_name.clone();
        let poll_cache = pa_sink_input.clone();

        let event_thread = std::thread::Builder::new()
            .name(thread_name.into())
            .spawn(move || {
                Self::event_loop(
                    mpv_clone,
                    run_flag,
                    load_flag,
                    sender,
                    poll_client,
                    poll_cache,
                );
            })
            .expect("Failed to spawn mpv event thread");

        info!("{init_log}");
        Self {
            mpv,
            raw_mpv_handle,
            running,
            loading,
            event_thread: Some(event_thread),
            has_af: AtomicBool::new(false),
            audio_client_name,
            pa_sink_input,
        }
    }

    fn event_loop(
        mpv: Arc<Mutex<MpvClient>>,
        running: Arc<AtomicBool>,
        loading: Arc<AtomicBool>,
        sender: async_channel::Sender<PlayerEvent>,
        audio_client_name: String,
        pa_sink_input: Arc<Mutex<Option<u32>>>,
    ) {
        let mut poll = PollState::default();

        while running.load(Ordering::Relaxed) {
            {
                let mut mpv_guard = mpv
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !Self::drain_event_queue(&mut mpv_guard, &running, &loading, &sender, &mut poll)
                {
                    return;
                }
                Self::poll_properties(&mpv_guard, &loading, &sender, &mut poll);
            }
            // Mirror external mixer changes (pavucontrol etc.) back to the UI.
            // Outside the mpv lock — it shells `pactl`, and only every ~700 ms.
            Self::poll_stream_volume(&audio_client_name, &pa_sink_input, &sender, &mut poll);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        debug!("MPV event loop exited");
    }

    /// Poll our sink-input's volume/mute (throttled) so a change made in the
    /// system mixer shows on the in-app slider. Diff-gated: only emits on change.
    fn poll_stream_volume(
        client_name: &str,
        cache: &Arc<Mutex<Option<u32>>>,
        sender: &async_channel::Sender<PlayerEvent>,
        poll: &mut PollState,
    ) {
        let now = std::time::Instant::now();
        if poll
            .last_volume_poll
            .is_some_and(|last| now.duration_since(last) < std::time::Duration::from_millis(700))
        {
            return;
        }
        poll.last_volume_poll = Some(now);

        // Single `pactl` query per tick for both volume and mute.
        if let Some((volume, mute)) = super::pactl_volume::get_state(cache, client_name) {
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

    /// Drain mpv's queued events. Returns `false` if the sender closed
    /// (caller must exit the event loop).
    fn drain_event_queue(
        mpv_guard: &mut MpvClient,
        running: &Arc<AtomicBool>,
        loading: &Arc<AtomicBool>,
        sender: &async_channel::Sender<PlayerEvent>,
        poll: &mut PollState,
    ) -> bool {
        while let Some(player_event) = Self::next_event(mpv_guard, loading, poll) {
            if matches!(player_event, PlayerEvent::Shutdown) {
                running.store(false, Ordering::Relaxed);
            }
            if sender.send_blocking(player_event).is_err() {
                running.store(false, Ordering::Relaxed);
                return false;
            }
        }
        true
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
        }
    }

    /// Poll time/duration/pause/eof/title/buffering and dispatch diff-based
    /// `PlayerEvent`s. Runs at ~20 Hz from the main event_loop.
    fn poll_properties(
        mpv_guard: &MpvClient,
        loading: &Arc<AtomicBool>,
        sender: &async_channel::Sender<PlayerEvent>,
        poll: &mut PollState,
    ) {
        for pending in poll.pending.drain(..) {
            let _ = sender.send_blocking(pending);
        }
        if let Ok(pos) = mpv_guard.get_property::<f64>("time-pos")
            && (pos - poll.last_pos).abs() > 0.05
        {
            poll.last_pos = pos;
            let _ = sender.try_send(PlayerEvent::PositionChanged(pos));
        }
        if let Ok(dur) = mpv_guard.get_property::<f64>("duration")
            && (dur - poll.last_dur).abs() > 0.1
        {
            poll.last_dur = dur;
            let _ = sender.try_send(PlayerEvent::DurationChanged(dur));
        }
        if let Ok(paused) = Self::get_bool_property(mpv_guard, "pause")
            && poll.last_paused != Some(paused)
        {
            poll.last_paused = Some(paused);
            let _ = sender.try_send(PlayerEvent::PauseChanged(paused));
        }
        if loading.load(Ordering::Relaxed) {
            log::debug!("[MPV-POLL] skipping eof-reached check (loading=true)");
        } else if let Ok(eof) = Self::get_bool_property(mpv_guard, "eof-reached") {
            if eof && !poll.last_eof {
                log::debug!("[MPV-POLL] eof-reached=true, sending synthetic EndFile(0)");
                let _ = sender.try_send(PlayerEvent::EndFile(0));
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
            let _ = sender.try_send(PlayerEvent::MetadataChanged(meta));
        }
        if let Ok(buffering) = Self::get_bool_property(mpv_guard, "paused-for-cache")
            && poll.last_buffering != Some(buffering)
        {
            poll.last_buffering = Some(buffering);
            let _ = sender.try_send(PlayerEvent::Buffering(buffering));
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
        if let Some(handle) = self.event_thread.take() {
            let _ = handle.join();
        }

        self.mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .terminate_destroy();

        info!("MpvBackend dropped (mpv_terminate_destroy)");
    }
}
