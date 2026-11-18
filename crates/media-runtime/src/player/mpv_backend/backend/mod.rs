//! `MpvBackend`: mpv ownership, background event loop, lifecycle.
//! Per-mode initialization options live in `init_options`.

mod client;
mod init_options;
mod volume;

pub use client::MpvError;
use client::{MpvClient, MpvEvent};
use log::{debug, info};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{EndReason, PlayerEvent, TrackMetadata};
use init_options::{
    AUDIO_WATCH_LATER_OPTIONS, VIDEO_WATCH_LATER_OPTIONS, apply_audio_only_options,
    apply_audio_output, apply_custom_options, apply_playback_options, apply_video_options,
    apply_watch_later, parse_custom_options,
};

/// Last-seen property values of the event loop, plus events produced while
/// draining that must follow the properties polled in the same pass (so the
/// UI sees a coherent order on FileLoaded).
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
    /// An observed playback property may have changed since the last read.
    state_changed: bool,
    /// A file is loaded and reports a playback position.
    has_position: bool,
    /// No file is loaded (`idle-active`).
    idle: bool,
    pending: Vec<PlayerEvent>,
}

impl PollState {
    fn reset_on_load(&mut self) {
        self.last_pos = -1.0;
        self.last_dur = -1.0;
        self.last_eof = false;
        self.seek_pending = false;
        self.state_changed = true;
    }

    /// Only a playing file changes without mpv queuing an event: the position.
    fn playing(&self) -> bool {
        !self.idle && self.has_position && self.last_paused == Some(false) && !self.last_eof
    }
}

/// `observe_property` group of the properties the volume controls follow.
const VOLUME_PROPERTIES: u64 = 0;
/// `observe_property` group of the properties `poll_properties` reads.
const PLAYBACK_PROPERTIES: u64 = 1;

pub struct MpvBackend {
    pub(in crate::player::mpv_backend) hwdec_before_filters: Mutex<Option<String>>,
    /// The render context draws with a software GL, so the CPU decodes.
    #[cfg(feature = "video")]
    pub(in crate::player::mpv_backend) software_gl: AtomicBool,
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
    /// Userdata of mpv's wakeup callback. Dropped after Drop has run
    /// mpv_terminate_destroy, the last point mpv can call it.
    _wakeup: Box<SyncSender<()>>,
    /// Tracks whether audio filters are currently set, so we can skip
    /// the expensive ao-reload when clearing an already-empty chain.
    pub(in crate::player::mpv_backend) has_af: AtomicBool,
    pub(in crate::player::mpv_backend) stream_volume: volume::VolumeControl,
    /// The person's subtitle distance from the bottom, in mpv's scaled pixels.
    pub(in crate::player::mpv_backend) sub_margin_base: AtomicI64,
    /// Scaled pixels added to `sub_margin_base` while controls cover the bottom.
    pub(in crate::player::mpv_backend) sub_margin_lift: AtomicI64,
}

// SAFETY: raw_mpv_handle points to an mpv_handle that lives as long as the
// Arc<Mutex<MpvClient>>; mpv_command_async and mpv_wakeup are thread-safe on it.
unsafe impl Send for MpvBackend {}
unsafe impl Sync for MpvBackend {}

/// mpv calls this from its own threads whenever it queues an event.
unsafe extern "C" fn wake_event_loop(sender: *mut c_void) {
    // SAFETY: `sender` is the backend's boxed SyncSender, which outlives
    // mpv_terminate_destroy. try_send neither blocks nor calls into mpv.
    let sender = unsafe { &*sender.cast::<SyncSender<()>>() };
    let _ = sender.try_send(());
}

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
        let volume_properties = [
            "current-ao",
            "audio-device",
            "ao-volume",
            "ao-mute",
            "volume",
            "mute",
        ]
        .map(|property| (property, VOLUME_PROPERTIES));
        let playback_properties = [
            "idle-active",
            "duration",
            "pause",
            "eof-reached",
            "speed",
            "media-title",
            "paused-for-cache",
        ]
        .map(|property| (property, PLAYBACK_PROPERTIES));
        for (property, group) in volume_properties.into_iter().chain(playback_properties) {
            if let Err(error) = mpv.observe_property(property, group) {
                log::error!("Cannot observe {property}: {error}");
            }
        }
        let raw_mpv_handle = mpv.raw_handle();
        let (wake_sender, wake) = std::sync::mpsc::sync_channel(1);
        let wakeup = Box::new(wake_sender);
        // SAFETY: the handle is live and `wakeup` outlives it (see `_wakeup`).
        unsafe {
            mpv_sys::mpv_set_wakeup_callback(
                raw_mpv_handle,
                Some(wake_event_loop),
                std::ptr::from_ref::<SyncSender<()>>(&wakeup)
                    .cast_mut()
                    .cast(),
            );
        }
        let mpv = Arc::new(Mutex::new(mpv));
        let running = Arc::new(AtomicBool::new(true));
        let loading = Arc::new(AtomicBool::new(false));
        let stream_volume = volume::VolumeControl::new();
        let screenshots = Arc::new(super::screenshots::Screenshots::new());
        let event_loop = EventLoop {
            mpv: mpv.clone(),
            running: running.clone(),
            loading: loading.clone(),
            sender: sender.clone(),
            volume: stream_volume.clone(),
            screenshots: screenshots.clone(),
            wake,
        };
        let event_thread = std::thread::Builder::new()
            .name(thread_name.into())
            .spawn(move || event_loop.run())
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
            event_sender: sender,
            _wakeup: wakeup,
            has_af: AtomicBool::new(false),
            hwdec_before_filters: Mutex::new(None),
            #[cfg(feature = "video")]
            software_gl: AtomicBool::new(false),
            stream_volume,
            sub_margin_base: AtomicI64::new(0),
            sub_margin_lift: AtomicI64::new(0),
        }
    }

    /// Make the event loop run a pass now, also when it waits with nothing loaded.
    pub(in crate::player::mpv_backend) fn wake_event_loop(&self) {
        // SAFETY: mpv_wakeup is thread-safe and the handle is live until Drop.
        unsafe { mpv_sys::mpv_wakeup(self.raw_mpv_handle) };
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

    /// Report the track list again after a command changed it outside a file
    /// load, which is the only time the event loop reads it.
    #[cfg(feature = "video")]
    pub(in crate::player::mpv_backend) fn republish_tracks(&self) {
        let tracks = self.get_track_list();
        let _ = self
            .event_sender
            .try_send(PlayerEvent::TracksChanged(tracks));
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
                format!("{} {}", crate::i18n::i18n("Chapter"), i + 1)
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
}

/// The background thread's share of the backend.
struct EventLoop {
    mpv: Arc<Mutex<MpvClient>>,
    running: Arc<AtomicBool>,
    loading: Arc<AtomicBool>,
    sender: async_channel::Sender<PlayerEvent>,
    volume: volume::VolumeControl,
    screenshots: Arc<super::screenshots::Screenshots>,
    /// Signalled by mpv's wakeup callback and by `MpvBackend::wake_event_loop`.
    wake: Receiver<()>,
}

impl EventLoop {
    fn run(self) {
        let mut poll = PollState {
            volume_changed: true,
            state_changed: true,
            ..PollState::default()
        };
        let mut events = Vec::with_capacity(80);
        let handle = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .raw_handle();

        while self.running.load(Ordering::Relaxed) && !self.sender.is_closed() {
            for id in self.screenshots.withdraw_expired() {
                // SAFETY: Drop joins this thread before destroying the handle.
                // The registry still owns the staging file until terminal receipt.
                unsafe {
                    mpv_sys::mpv_abort_async_command(handle, id);
                }
                events.push(PlayerEvent::ScreenshotTimedOut { id });
            }
            let drained = {
                let mut guard = self
                    .mpv
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let drained = drain_event_queue(&mut guard, &self.loading, &mut poll, &mut events);
                poll_properties(&guard, &self.loading, &mut poll, &mut events);
                self.sync_volume(&guard, &mut poll, &mut events);
                drained
            };
            if !self.deliver(&mut events) {
                return;
            }
            let playing = poll.playing();
            if drained && !playing && !self.screenshots.has_requests() {
                // Paused, ended or with nothing loaded, nothing changes until
                // mpv queues an event or the UI asks for a volume change.
                if self.wake.recv().is_err() {
                    break;
                }
                continue;
            }
            // Every position change redraws the window: ten a second keep the
            // seek bar moving smoothly. Otherwise only screenshot deadlines
            // need a timer.
            let interval = if playing { 100 } else { 250 };
            // Wait on mpv's wakeup rather than sleeping: an event (a volume
            // change) is handled at once, and Drop's wakeup ends the wait
            // instead of the join sitting out the rest of the interval.
            if let Err(std::sync::mpsc::RecvTimeoutError::Disconnected) =
                self.wake.recv_timeout(Duration::from_millis(interval))
            {
                break;
            }
        }
        debug!("MPV event loop exited");
    }

    fn sync_volume(&self, mpv: &MpvClient, poll: &mut PollState, events: &mut Vec<PlayerEvent>) {
        let Some((volume, mute)) = self.volume.sync(
            mpv,
            std::mem::take(&mut poll.volume_changed),
            std::mem::take(&mut poll.audio_reconfigured),
            poll.playing(),
        ) else {
            return;
        };
        if poll
            .last_volume
            .is_none_or(|last| (last - volume).abs() > 0.01)
        {
            poll.last_volume = Some(volume);
            events.push(PlayerEvent::VolumeChanged(volume));
        }
        if poll.last_mute != Some(mute) {
            poll.last_mute = Some(mute);
            events.push(PlayerEvent::MuteChanged(mute));
        }
    }

    /// Hand the pass's events to the UI. Called without the mpv mutex: a UI
    /// handler may need it before it can drain the bounded channel. Returns
    /// `false` once the loop must stop.
    fn deliver(&self, events: &mut Vec<PlayerEvent>) -> bool {
        for event in events.drain(..) {
            let mut event = match event {
                PlayerEvent::AsyncCommandReply { id, error } => {
                    let Some(result) = self.screenshots.complete(id, error) else {
                        continue;
                    };
                    PlayerEvent::ScreenshotFinished { id, result }
                }
                other => other,
            };
            loop {
                match self.sender.try_send(event) {
                    Ok(()) => break,
                    Err(async_channel::TrySendError::Full(back))
                        if self.running.load(Ordering::Relaxed) =>
                    {
                        event = back;
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => return false,
                }
            }
        }
        true
    }
}

/// Collect owned events under the mpv lock, without sending or waiting.
/// A bounded batch prevents an event burst monopolising the client mutex.
/// Returns whether mpv's queue was emptied: only then does mpv call the
/// wakeup callback for the next event.
fn drain_event_queue(
    mpv: &mut MpvClient,
    loading: &AtomicBool,
    poll: &mut PollState,
    events: &mut Vec<PlayerEvent>,
) -> bool {
    for _ in 0..64 {
        let Some(event) = next_event(mpv, loading, poll) else {
            return true;
        };
        let shutdown = matches!(&event, PlayerEvent::Shutdown);
        events.push(event);
        if shutdown {
            break;
        }
    }
    false
}

/// Pull the next event worth reporting; `None` once mpv's queue is empty.
/// Lifecycle events update `loading` and `poll` on the way.
fn next_event(
    mpv: &mut MpvClient,
    loading: &AtomicBool,
    poll: &mut PollState,
) -> Option<PlayerEvent> {
    loop {
        let event = match mpv.wait_event(0.0)? {
            Ok(event) => event,
            Err(MpvError::Raw(_)) => continue,
            Err(_) => return None,
        };
        match event {
            MpvEvent::AudioReconfig => {
                poll.volume_changed = true;
                poll.audio_reconfigured = true;
            }
            MpvEvent::PropertyChange(VOLUME_PROPERTIES) => poll.volume_changed = true,
            MpvEvent::PropertyChange(_) => poll.state_changed = true,
            MpvEvent::Seek => poll.seek_pending = true,
            MpvEvent::PlaybackRestart => {
                if std::mem::take(&mut poll.seek_pending)
                    && let Ok(position) = mpv.get_property::<f64>("time-pos")
                    && position.is_finite()
                    && position >= 0.0
                {
                    return Some(PlayerEvent::Seeked(position));
                }
            }
            MpvEvent::FileLoaded => {
                loading.store(false, Ordering::Relaxed);
                poll.reset_on_load();
                let title = mpv
                    .get_property::<String>("media-title")
                    .unwrap_or_default();
                if let Ok(tracks) = MpvBackend::get_track_list_locked(mpv) {
                    poll.pending.push(PlayerEvent::TracksChanged(tracks));
                }
                if let Ok(chapters) = MpvBackend::get_chapters_locked(mpv) {
                    poll.pending.push(PlayerEvent::ChaptersChanged(chapters));
                }
                return Some(PlayerEvent::FileLoaded(title));
            }
            MpvEvent::EndFile(reason) => {
                poll.seek_pending = false;
                poll.last_eof = true;
                return Some(PlayerEvent::EndFile(reason));
            }
            MpvEvent::Shutdown => return Some(PlayerEvent::Shutdown),
            MpvEvent::CommandReply { id: 0, error } => {
                if error < 0 {
                    log::error!(
                        "Asynchronous playback command failed: {}",
                        MpvError::Raw(error)
                    );
                }
            }
            MpvEvent::CommandReply { id, error } => {
                return Some(PlayerEvent::AsyncCommandReply { id, error });
            }
            MpvEvent::Other => {}
        }
    }
}

/// Read the position, and the observed playback properties when mpv reported
/// a change, pushing diff-based `PlayerEvent`s once per pass of the event loop.
fn poll_properties(
    mpv: &MpvClient,
    loading: &AtomicBool,
    poll: &mut PollState,
    events: &mut Vec<PlayerEvent>,
) {
    events.append(&mut poll.pending);
    let position = mpv.get_property::<f64>("time-pos");
    poll.has_position = position.is_ok();
    if let Ok(pos) = position
        && (pos - poll.last_pos).abs() > 0.05
    {
        poll.last_pos = pos;
        events.push(PlayerEvent::PositionChanged(pos));
    }
    if !std::mem::take(&mut poll.state_changed) {
        return;
    }
    poll.idle = mpv.get_property::<bool>("idle-active").unwrap_or(false);
    if let Ok(dur) = mpv.get_property::<f64>("duration")
        && (dur - poll.last_dur).abs() > 0.1
    {
        poll.last_dur = dur;
        events.push(PlayerEvent::DurationChanged(dur));
    }
    if let Ok(paused) = mpv.get_property::<bool>("pause")
        && poll.last_paused != Some(paused)
    {
        poll.last_paused = Some(paused);
        events.push(PlayerEvent::PauseChanged(paused));
    }
    // While a file loads, eof-reached still describes the previous one.
    if !loading.load(Ordering::Relaxed)
        && let Ok(eof) = mpv.get_property::<bool>("eof-reached")
    {
        if eof && !poll.last_eof {
            events.push(PlayerEvent::EndFile(EndReason::Eof));
        }
        poll.last_eof = eof;
    }
    if let Ok(rate) = mpv.get_property::<f64>("speed")
        && rate.is_finite()
        && rate > 0.0
        && poll.last_rate != Some(rate)
    {
        poll.last_rate = Some(rate);
        events.push(PlayerEvent::RateChanged(rate));
    }
    if let Ok(title) = mpv.get_property::<String>("media-title")
        && !title.is_empty()
        && title != poll.last_title
    {
        poll.last_title.clone_from(&title);
        let mut meta = HashMap::new();
        meta.insert("title".to_string(), title);
        if let Ok(artist) = mpv.get_property::<String>("metadata/by-key/artist") {
            meta.insert("artist".to_string(), artist);
        }
        events.push(PlayerEvent::MetadataChanged(meta));
    }
    if let Ok(buffering) = mpv.get_property::<bool>("paused-for-cache")
        && poll.last_buffering != Some(buffering)
    {
        poll.last_buffering = Some(buffering);
        events.push(PlayerEvent::Buffering(buffering));
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
        // inside Drop: closing fails its next send, and the wakeup ends a wait
        // with nothing loaded.
        self.event_sender.close();
        self.wake_event_loop();
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
mod tests;
