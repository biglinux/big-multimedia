//! `MpvBackend` — mpv ownership, background event loop, lifecycle.
//! Per-mode initialization options live in `init_options`. Split from
//! the original monolithic `backend.rs`; public API is unchanged.

mod client;
mod init_options;
mod volume;

pub use client::MpvError;
use client::{MpvClient, MpvEvent, MpvPropertyValue};
use log::{debug, info};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::{PlayerEvent, TrackMetadata};
use init_options::{
    AUDIO_WATCH_LATER_OPTIONS, VIDEO_WATCH_LATER_OPTIONS, apply_audio_only_options,
    apply_audio_output, apply_custom_options, apply_input_ipc, apply_playback_options,
    apply_video_options, apply_watch_later, parse_custom_options,
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
    volume_changed: bool,
    audio_reconfigured: bool,
    last_rate: Option<f64>,
    seek_pending: bool,
    pending: Vec<PlayerEvent>,
}

impl PollState {
    fn reset_on_load(&mut self) {
        self.last_pos = -1.0;
        self.last_dur = -1.0;
        self.last_eof = false;
        self.seek_pending = false;
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
    CommandReply { id: u64, error: i32 },
}

pub struct MpvBackend {
    pub(in crate::player::mpv_backend) hwdec_before_filters: Mutex<Option<String>>,
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
    pub(in crate::player::mpv_backend) stream_volume: volume::VolumeControl,
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
            apply_watch_later(
                init,
                crate::config::video::WATCH_LATER_RELATIVE,
                VIDEO_WATCH_LATER_OPTIONS,
            )?;
            apply_input_ipc(init, crate::config::video::MPV_IPC_SOCKET);
            apply_custom_options(init, &custom_opts);
            Ok(())
        })?;

        Ok(Self::finish_mpv_init(
            mpv,
            sender,
            "mpv-events",
            "MpvBackend initialized",
            false,
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
            apply_watch_later(
                init,
                crate::config::audio::WATCH_LATER_RELATIVE,
                AUDIO_WATCH_LATER_OPTIONS,
            )?;
            Ok(())
        })?;

        Ok(Self::finish_mpv_init(
            mpv,
            sender,
            "mpv-audio-events",
            "MpvBackend initialized (audio-only)",
            true,
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
        let stream_volume = volume::VolumeControl::new();
        let volume_state = stream_volume.clone();
        for property in [
            "current-ao",
            "audio-device",
            "ao-volume",
            "ao-mute",
            "volume",
            "mute",
        ] {
            if let Err(error) = mpv
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .observe_property(property)
            {
                log::error!("Cannot observe {property}: {error}");
            }
        }

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
            hwdec_before_filters: Mutex::new(None),
            stream_volume,
        }
    }

    fn event_loop(
        mpv: Arc<Mutex<MpvClient>>,
        running: Arc<AtomicBool>,
        loading: Arc<AtomicBool>,
        sender: async_channel::Sender<PlayerEvent>,
        volume_state: volume::VolumeControl,
        screenshots: Arc<super::screenshots::Screenshots>,
    ) {
        let mut poll = PollState {
            volume_changed: true,
            ..PollState::default()
        };
        let mut outgoing = Vec::with_capacity(80);
        let mut pending = std::collections::VecDeque::with_capacity(96);
        let handle = mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .raw_handle();

        while running.load(Ordering::Relaxed) && !sender.is_closed() {
            for id in screenshots.withdraw_expired() {
                // The registry still owns the staging file until terminal receipt.
                unsafe {
                    mpv_sys::mpv_abort_async_command(handle, id);
                }
                pending.push_back(PlayerEvent::ScreenshotTimedOut { id });
            }
            while let Some(event) = pending.pop_front() {
                match sender.try_send(event) {
                    Ok(()) => {}
                    Err(async_channel::TrySendError::Full(event)) => {
                        pending.push_front(event);
                        break;
                    }
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
                if let Some((volume, mute)) = volume_state.sync(
                    &guard,
                    std::mem::take(&mut poll.volume_changed),
                    std::mem::take(&mut poll.audio_reconfigured),
                ) {
                    if poll
                        .last_volume
                        .is_none_or(|last| (last - volume).abs() > 0.01)
                    {
                        poll.last_volume = Some(volume);
                        outgoing.push(PlayerEvent::VolumeChanged(volume));
                    }
                    if poll.last_mute != Some(mute) {
                        poll.last_mute = Some(mute);
                        outgoing.push(PlayerEvent::MuteChanged(mute));
                    }
                }
            }
            // Never wait for the UI while holding the mpv mutex: a UI handler
            // may need that same mutex before it can drain the bounded channel.
            for event in outgoing.drain(..) {
                let event = match event {
                    PlayerEvent::AsyncCommandReply { id, error } => {
                        let Some(result) = screenshots.complete(id, error) else {
                            continue;
                        };
                        PlayerEvent::ScreenshotFinished { id, result }
                    }
                    other => other,
                };
                pending.push_back(event);
            }
            // Position feedback stays smooth during playback, but a paused
            // player does not need to poll every property twenty times/second.
            let interval = if poll.last_paused == Some(true) {
                200
            } else {
                50
            };
            std::thread::sleep(std::time::Duration::from_millis(interval));
        }
        debug!("MPV event loop exited");
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
                Ok(MpvEvent::AudioReconfig) => {
                    poll.volume_changed = true;
                    poll.audio_reconfigured = true;
                    continue;
                }
                Ok(MpvEvent::Seek) => {
                    poll.seek_pending = true;
                    continue;
                }
                Ok(MpvEvent::PlaybackRestart) => {
                    if std::mem::take(&mut poll.seek_pending)
                        && let Ok(position) = mpv_guard.get_property::<f64>("time-pos")
                        && position.is_finite()
                        && position >= 0.0
                    {
                        return Some(PlayerEvent::Seeked(position));
                    }
                    continue;
                }
                Ok(MpvEvent::FileLoaded) => MappedEvent::FileLoaded,
                Ok(MpvEvent::EndFile(reason)) => MappedEvent::EndFile(reason),
                Ok(MpvEvent::Shutdown) => MappedEvent::Shutdown,
                Ok(MpvEvent::CommandReply { id: 0, error }) => {
                    if error < 0 {
                        log::error!(
                            "Asynchronous playback command failed: {}",
                            MpvError::Raw(error)
                        );
                    }
                    continue;
                }
                Ok(MpvEvent::CommandReply { id, error }) => MappedEvent::CommandReply { id, error },
                Ok(MpvEvent::PropertyChange { name, change }) => match (name.as_str(), change) {
                    ("time-pos", MpvPropertyValue::Double(v)) => MappedEvent::Position(v),
                    ("duration", MpvPropertyValue::Double(v)) => MappedEvent::Duration(v),
                    ("pause", MpvPropertyValue::Flag(v)) => MappedEvent::Paused(v),
                    (
                        "current-ao" | "audio-device" | "ao-volume" | "ao-mute" | "volume" | "mute",
                        _,
                    ) => {
                        poll.volume_changed = true;
                        continue;
                    }
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
                poll.seek_pending = false;
                poll.last_eof = true;
                log::debug!("[MPV-EVENT] EndFile reason={reason} (from mpv event queue)");
                PlayerEvent::EndFile(reason)
            }
            MappedEvent::Shutdown => PlayerEvent::Shutdown,
            MappedEvent::Position(v) => PlayerEvent::PositionChanged(v),
            MappedEvent::Duration(v) => PlayerEvent::DurationChanged(v),
            MappedEvent::Paused(v) => PlayerEvent::PauseChanged(v),
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
        if let Ok(rate) = mpv_guard.get_property::<f64>("speed")
            && rate.is_finite()
            && rate > 0.0
            && poll.last_rate != Some(rate)
        {
            poll.last_rate = Some(rate);
            outgoing.push(PlayerEvent::RateChanged(rate));
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

impl MpvBackend {
    /// When resume is on, save the loaded file's position now and stop mpv
    /// from saving again on quit. mpv's own save runs only at teardown, after
    /// a stop has already unloaded the file, and would keep a finished file's
    /// end position; such a file is forgotten instead, as resuming there
    /// would reopen on its last frame and end at once.
    pub fn save_resume_position(&self) {
        self.with_mpv(|mpv| {
            if !mpv
                .get_property::<bool>("save-position-on-quit")
                .unwrap_or(false)
            {
                return;
            }
            let command = if mpv.get_property::<bool>("eof-reached").unwrap_or(false) {
                "delete-watch-later-config"
            } else {
                "write-watch-later-config"
            };
            let _ = mpv.command(command, &[]);
            let _ = mpv.set_property("save-position-on-quit", false);
        });
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

        self.save_resume_position();
        self.mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .terminate_destroy();
        self.screenshots.clear_after_termination();

        info!("MpvBackend dropped (mpv_terminate_destroy)");
    }
}

#[cfg(all(test, not(miri)))]
mod volume_tests {
    use super::*;
    use crate::player::mpv_backend::types::AudioCacheProfile;
    use std::time::{Duration, Instant};

    fn wait_until(label: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for {label}");
    }

    #[test]
    #[ignore = "requires BIG_MEDIA_TEST_AUDIO_ISOLATED=1, private PipeWire/Pulse and FFmpeg"]
    fn native_sound_server_volume_tracks_external_changes() {
        use big_os_kit::subprocess::BigSubprocessSpec;
        assert_eq!(
            std::env::var("BIG_MEDIA_TEST_AUDIO_ISOLATED").as_deref(),
            Ok("1")
        );
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("silence.wav");
        let generated = BigSubprocessSpec::builder()
            .program("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=stereo",
                "-t",
                "8",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&input)
            .timeout(Duration::from_secs(5))
            .build()
            .run()
            .unwrap();
        assert!(generated.status.success(), "{}", generated.stderr_lossy());
        for driver in ["pulse", "pipewire"] {
            let name = format!("review-volume-{}-{driver}", std::process::id());
            let mpv = MpvClient::with_initializer(|init| {
                apply_audio_only_options(init)?;
                init.set_option("audio-client-name", name.as_str())?;
                init.set_option("ao", driver)
            })
            .unwrap();
            let (tx, _rx) = async_channel::unbounded();
            let backend = MpvBackend::finish_mpv_init(
                mpv,
                tx,
                "test-native-volume",
                "test native volume",
                true,
            );
            backend.set_volume(37.0);
            backend.set_mute(true);
            wait_until("pending software volume", || {
                backend.get_property_f64("volume") == 37.0 && backend.get_property_bool("mute")
            });
            backend.load_file(input.to_str().unwrap());
            wait_until("native output and transferred controls", || {
                backend.get_current_ao() == driver
                    && (backend.get_volume() - 37.0).abs() < 0.1
                    && backend.get_property_bool("ao-mute")
                    && backend.get_property_f64("volume") == 100.0
                    && !backend.get_property_bool("mute")
            });
            let listed = BigSubprocessSpec::builder()
                .program("pactl")
                .args(["-f", "json", "list", "sink-inputs"])
                .timeout(Duration::from_secs(2))
                .build()
                .run()
                .unwrap();
            assert!(listed.status.success(), "{}", listed.stderr_lossy());
            let streams: Vec<serde_json::Value> = serde_json::from_slice(&listed.stdout).unwrap();
            let stream = streams
                .iter()
                .find(|s| s["properties"]["application.name"] == name)
                .expect("test stream visible to the sound server");
            assert_eq!(stream["mute"], true, "{driver}: actual server mute");
            let percent = stream["volume"]
                .as_object()
                .unwrap()
                .values()
                .next()
                .unwrap()["value_percent"]
                .as_str()
                .unwrap()
                .trim_end_matches('%')
                .parse::<f64>()
                .unwrap();
            assert!((percent - 37.0).abs() < 1.0, "{driver}: server={percent}%");
            let index = stream["index"].as_u64().unwrap().to_string();
            for (command, value) in [
                ("set-sink-input-volume", "72%"),
                ("set-sink-input-mute", "0"),
            ] {
                let set = BigSubprocessSpec::builder()
                    .program("pactl")
                    .args([command, &index, value])
                    .timeout(Duration::from_secs(2))
                    .build()
                    .run()
                    .unwrap();
                assert!(set.status.success(), "{}", set.stderr_lossy());
            }
            wait_until("external mixer changes", || {
                (backend.get_volume() - 72.0).abs() < 0.1 && !backend.get_mute()
            });
            assert_eq!(
                backend.get_property_f64("volume"),
                100.0,
                "{driver}: no double gain"
            );
            backend.set_volume_max(250);
            backend.set_volume(170.0);
            wait_until("amplified volume", || {
                (backend.get_property_f64("ao-volume") - 100.0).abs() < 0.1
                    && backend.get_property_f64("volume") == 170.0
            });
            backend.set_volume(72.0);
            backend.pause();
            std::thread::sleep(Duration::from_secs(1));
            let set = BigSubprocessSpec::builder()
                .program("pactl")
                .args(["set-sink-input-mute", &index, "1"])
                .timeout(Duration::from_secs(2))
                .build()
                .run()
                .unwrap();
            assert!(set.status.success(), "{}", set.stderr_lossy());
            wait_until("external mute while paused", || backend.get_mute());
            backend.set_mute(false);
            wait_until("unmute applied", || !backend.get_property_bool("ao-mute"));
            backend.stop();
            wait_until("output closed", || backend.get_current_ao().is_empty());
            backend.load_file(input.to_str().unwrap());
            wait_until("controls after reopening", || {
                backend.get_current_ao() == driver
                    && (backend.get_volume() - 72.0).abs() < 0.1
                    && !backend.get_mute()
            });
            eprintln!(
                "{driver}: 37% + mute reached the server; external72%/unmute and reopen preserved"
            );
        }
    }

    #[test]
    fn volume_and_mute_work_without_a_pulse_stream() {
        let mpv = MpvClient::with_initializer(|init| {
            apply_audio_only_options(init)?;
            init.set_option("ao", "null")
        })
        .unwrap();
        let (tx, rx) = async_channel::unbounded();
        let backend = MpvBackend::finish_mpv_init(mpv, tx, "test-volume", "test volume", true);
        for value in 0..1000 {
            backend.set_volume(f64::from(value % 100));
        }
        backend.set_volume(37.0);
        backend.set_mute(true);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if backend.get_property_f64("volume") == 37.0 && backend.get_property_bool("mute") {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(backend.get_property_f64("volume"), 37.0);
        assert!(backend.get_property_bool("mute"));
        assert_eq!(backend.get_volume(), 37.0);
        assert!(backend.get_mute());
        let mut volume_event = false;
        let mut mute_event = false;
        while Instant::now() < deadline && !(volume_event && mute_event) {
            for event in std::iter::from_fn(|| rx.try_recv().ok()) {
                volume_event |= matches!(event, PlayerEvent::VolumeChanged(v) if v == 37.0);
                mute_event |= matches!(event, PlayerEvent::MuteChanged(true));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(volume_event, "applied volume must reach the UI");
        assert!(mute_event, "applied mute must reach the UI");
    }

    #[test]
    fn close_saves_only_the_position_and_forgets_finished_files() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("silence.wav");
        // Four seconds of 8 kHz mono 16-bit silence.
        let data_len: u32 = 8000 * 2 * 4;
        let mut wav = b"RIFF".to_vec();
        wav.extend((36 + data_len).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        for field in [16_u32, 0x0001_0001, 8000, 16000, 0x0010_0002] {
            wav.extend(field.to_le_bytes());
        }
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        wav.resize(wav.len() + data_len as usize, 0);
        std::fs::write(&input, wav).unwrap();
        let input = input.to_str().unwrap();
        let watch_dir = temp.path().join("watch_later");
        let open = || {
            let mpv = MpvClient::with_initializer(|init| {
                apply_audio_only_options(init)?;
                init.set_option("ao", "null")?;
                init.set_option("watch-later-directory", watch_dir.to_str().unwrap())?;
                init.set_option("watch-later-options", AUDIO_WATCH_LATER_OPTIONS)
            })
            .unwrap();
            let (tx, rx) = async_channel::unbounded();
            let backend = MpvBackend::finish_mpv_init(mpv, tx, "test-resume", "test resume", true);
            backend.set_save_position(true);
            (backend, rx)
        };
        // Settings lines across the directory; mpv also writes comment-only
        // redirect entries for the parent directories.
        let saved = || -> Vec<String> {
            std::fs::read_dir(&watch_dir)
                .map(|entries| {
                    entries
                        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
                        .flat_map(|text| {
                            text.lines()
                                .filter(|line| !line.starts_with('#'))
                                .map(str::to_owned)
                                .collect::<Vec<_>>()
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        let (backend, _rx) = open();
        backend.load_audio_paused(input, AudioCacheProfile::Music, true);
        // Seek once audio output is up; sent earlier, mpv sometimes drops it.
        wait_until("playback started", || !backend.get_current_ao().is_empty());
        backend.seek_absolute(1.5);
        backend.with_mpv(|m| m.set_property("af", "volume=0.5".to_string()).unwrap());
        wait_until("seek", || {
            (backend.get_property_f64("time-pos") - 1.5).abs() < 0.01
        });
        drop(backend);
        assert_eq!(saved(), ["start=1.500000"], "filters stay with the app");

        let (backend, _rx) = open();
        backend.load_audio_paused(input, AudioCacheProfile::Music, true);
        wait_until("resumed", || {
            (backend.get_property_f64("time-pos") - 1.5).abs() < 0.01
        });
        backend.seek_absolute(4.0);
        wait_until("end of file", || backend.get_property_bool("eof-reached"));
        drop(backend);
        assert!(
            saved().is_empty(),
            "a finished file must not resume at its end"
        );
    }
}
