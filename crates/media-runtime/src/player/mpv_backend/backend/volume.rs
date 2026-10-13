//! Per-application sound-server control, with software gain for other outputs.
//! All mpv calls run on the backend's event thread; GTK only updates pending
//! values or reads the snapshot.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::client::MpvClient;

#[derive(Clone, Copy)]
struct State {
    volume: f64,
    mute: bool,
    pending_volume: Option<f64>,
    pending_mute: Option<bool>,
    volume_set: bool,
    mute_set: bool,
    had_output: bool,
    native_volume: bool,
    native_mute: bool,
    last_sync: Instant,
    /// Until then a 0 volume is not the user's: a stream the server has just
    /// created reports 0 before it applies ours, and mpv opens a new stream on
    /// every output rate change (an effect chain that resamples).
    new_stream_until: Instant,
}

/// How long a new stream's reported 0 volume is not taken as the user's.
const NEW_STREAM: Duration = Duration::from_secs(1);

/// The stream's volume on our 0–250 scale, amplification included.
fn read_volume(mpv: &MpvClient, native_volume: bool, pipewire: bool) -> Option<f64> {
    let property = if native_volume { "ao-volume" } else { "volume" };
    let volume = mpv
        .get_property::<f64>(property)
        .ok()
        .filter(|v| v.is_finite())?;
    if !native_volume {
        return Some(volume);
    }
    let level = if pipewire {
        100.0 * (volume / 100.0).cbrt()
    } else {
        volume
    };
    Some(level * mpv.get_property::<f64>("volume").unwrap_or(100.0) / 100.0)
}

fn read_mute(mpv: &MpvClient, native_mute: bool) -> Option<bool> {
    let property = if native_mute { "ao-mute" } else { "mute" };
    let mute = mpv.get_property::<bool>(property).ok()?;
    Some(mute || (native_mute && mpv.get_property::<bool>("mute").unwrap_or(false)))
}

/// Write the slider's `volume` to the stream's control. `transition` re-applies
/// the software gain after the control changed. `false` when mpv refused it.
fn write_volume(
    mpv: &MpvClient,
    volume: Option<f64>,
    native_volume: bool,
    pipewire: bool,
    transition: bool,
) -> bool {
    if let Some(volume) = volume {
        // mpv exposes PipeWire's linear amplitude; PulseAudio and desktop
        // mixers use a cubic scale. Keep the same slider loudness on both.
        let level = if native_volume && pipewire {
            100.0 * (volume.min(100.0) / 100.0).powi(3)
        } else if native_volume {
            volume.min(100.0)
        } else {
            volume.min(mpv.get_property::<f64>("volume-max").unwrap_or(150.0))
        };
        let property = if native_volume { "ao-volume" } else { "volume" };
        if let Err(error) = mpv.set_property(property, level) {
            log::warn!("Cannot change playback volume: {error}");
            return false;
        }
    }
    if native_volume && (transition || volume.is_some()) {
        // ao-volume is capped at 100 by mpv. Apply only amplification in
        // software; ordinary levels must not be multiplied a second time.
        let gain = volume
            .unwrap_or(100.0)
            .max(100.0)
            .min(mpv.get_property::<f64>("volume-max").unwrap_or(150.0));
        if let Err(error) = mpv.set_property("volume", gain) {
            log::warn!("Cannot change playback amplification: {error}");
        }
    }
    true
}

/// Write `mute` to the stream's control, releasing mpv's software mute when
/// the server's control takes over. `false` when mpv refused it.
fn write_mute(mpv: &MpvClient, mute: Option<bool>, native_mute: bool, transition: bool) -> bool {
    if let Some(mute) = mute {
        let property = if native_mute { "ao-mute" } else { "mute" };
        if let Err(error) = mpv.set_property(property, mute) {
            log::warn!("Cannot change playback mute: {error}");
            return false;
        }
    }
    if native_mute
        && (transition || mute.is_some())
        && let Err(error) = mpv.set_property("mute", false)
    {
        log::warn!("Cannot release software mute: {error}");
    }
    true
}

#[derive(Clone)]
pub(in crate::player::mpv_backend) struct VolumeControl(Arc<Mutex<State>>);

impl VolumeControl {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(State {
            volume: 100.0,
            mute: false,
            pending_volume: None,
            pending_mute: None,
            volume_set: false,
            // Set from the start: the first output gets "not muted" from us.
            // The sound server restores each application's last mute when its
            // stream appears (WirePlumber keeps it per application), and a
            // player closed muted would open muted with nothing on screen
            // saying so.
            mute_set: true,
            had_output: false,
            native_volume: false,
            native_mute: false,
            last_sync: Instant::now(),
            new_stream_until: Instant::now(),
        })))
    }

    pub fn snapshot(&self) -> (f64, bool) {
        let state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        (
            state.pending_volume.unwrap_or(state.volume),
            state.pending_mute.unwrap_or(state.mute),
        )
    }

    pub fn set_volume(&self, volume: f64) {
        if volume.is_finite() {
            let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            state.pending_volume = Some(volume.clamp(0.0, 250.0));
            state.volume_set = true;
        }
    }

    pub fn set_mute(&self, mute: bool) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.pending_mute = Some(mute);
        state.mute_set = true;
    }

    /// Whether `sync` reads the sound server's mixer on a timer: mpv does not
    /// reliably notify native mixer changes.
    pub(super) fn polls_mixer(&self) -> bool {
        let state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.native_volume || state.native_mute
    }

    /// The state to sync from, with its requests taken, or `None` when there
    /// is nothing to do.
    fn take_requests(&self, changed: bool) -> Option<State> {
        let mut shared = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let refresh = (shared.native_volume || shared.native_mute)
            && shared.last_sync.elapsed() >= Duration::from_millis(500);
        if !changed && !refresh && shared.pending_volume.is_none() && shared.pending_mute.is_none()
        {
            return None;
        }
        let snapshot = *shared;
        shared.pending_volume = None;
        shared.pending_mute = None;
        Some(snapshot)
    }

    pub(super) fn sync(
        &self,
        mpv: &MpvClient,
        changed: bool,
        reconfigured: bool,
    ) -> Option<(f64, bool)> {
        // Never keep the snapshot mutex locked across sound-server I/O: GTK
        // reads it for controls and can enqueue the next request concurrently.
        let mut state = self.take_requests(changed)?;
        let driver = mpv.get_property::<String>("current-ao").unwrap_or_default();
        // ALSA's ao-volume can control the system-wide mixer. Only these two
        // APIs promise a per-stream control belonging to this mpv instance.
        let per_stream = matches!(driver.as_str(), "pulse" | "pipewire");
        let native_volume = per_stream && mpv.get_property::<f64>("ao-volume").is_ok();
        let native_mute = per_stream && mpv.get_property::<bool>("ao-mute").is_ok();
        let pipewire = driver == "pipewire";
        let volume_transition = native_volume != state.native_volume || reconfigured;
        let mute_transition = native_mute != state.native_mute || reconfigured;
        let volume = state.pending_volume.take().or_else(|| {
            (volume_transition && (state.volume_set || state.had_output)).then_some(state.volume)
        });
        let mute = state.pending_mute.take().or_else(|| {
            (mute_transition && (state.mute_set || state.had_output)).then_some(state.mute)
        });
        let volume_applied = write_volume(mpv, volume, native_volume, pipewire, volume_transition);
        let mute_applied = write_mute(mpv, mute, native_mute, mute_transition);
        if native_volume && volume_transition {
            state.new_stream_until = Instant::now() + NEW_STREAM;
        }
        // What we just wrote is the value; reading it back in the same pass
        // returns what the server had before applying it.
        if let Some(requested) = volume.filter(|_| volume_applied) {
            let limit = mpv.get_property::<f64>("volume-max").unwrap_or(150.0);
            state.volume = requested.min(limit);
        } else if let Some(read) = read_volume(mpv, native_volume, pipewire)
            && (read > 0.0 || Instant::now() >= state.new_stream_until)
        {
            state.volume = read;
        }
        if let Some(requested) = mute.filter(|_| mute_applied) {
            state.mute = requested;
        } else if let Some(read) = read_mute(mpv, native_mute) {
            state.mute = read;
        }
        let mut shared = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        shared.volume = state.volume;
        shared.mute = state.mute;
        shared.native_volume = native_volume;
        shared.native_mute = native_mute;
        shared.had_output |= !driver.is_empty();
        shared.last_sync = Instant::now();
        shared.new_stream_until = state.new_stream_until;
        Some((state.volume, state.mute))
    }
}
