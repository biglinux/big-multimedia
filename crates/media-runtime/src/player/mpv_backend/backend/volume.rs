//! Per-application sound-server control, with software gain for other outputs.
//! All mpv calls run on its existing event thread. GTK only updates pending
//! values or reads the snapshot; there is no subprocess or separate worker.

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
            // stream appears (WirePlumber keeps it per application), so a
            // player closed muted came back muted the next day with nothing
            // on screen saying so.
            mute_set: true,
            had_output: false,
            native_volume: false,
            native_mute: false,
            last_sync: Instant::now(),
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

    pub(super) fn sync(
        &self,
        mpv: &MpvClient,
        changed: bool,
        reconfigured: bool,
    ) -> Option<(f64, bool)> {
        let mut state = {
            let mut shared = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            // Native mixer changes are not reliably notified by mpv. Refresh
            // active streams on the existing thread, including while paused.
            let refresh = (shared.native_volume || shared.native_mute)
                && shared.last_sync.elapsed() >= Duration::from_millis(500);
            if !changed
                && !refresh
                && shared.pending_volume.is_none()
                && shared.pending_mute.is_none()
            {
                return None;
            }
            let snapshot = *shared;
            shared.pending_volume = None;
            shared.pending_mute = None;
            snapshot
        };
        // Never keep the snapshot mutex locked across sound-server I/O: GTK
        // reads it for controls and can enqueue the next request concurrently.
        let driver = mpv.get_property::<String>("current-ao").unwrap_or_default();
        // ALSA's ao-volume can control the system-wide mixer. Only these two
        // APIs promise a per-stream control belonging to this mpv instance.
        let per_stream = matches!(driver.as_str(), "pulse" | "pipewire");
        let native_volume = per_stream && mpv.get_property::<f64>("ao-volume").is_ok();
        let native_mute = per_stream && mpv.get_property::<bool>("ao-mute").is_ok();
        let volume_transition = native_volume != state.native_volume || reconfigured;
        let mute_transition = native_mute != state.native_mute || reconfigured;
        let volume = state.pending_volume.take().or_else(|| {
            (volume_transition && (state.volume_set || state.had_output)).then_some(state.volume)
        });
        let mute = state.pending_mute.take().or_else(|| {
            (mute_transition && (state.mute_set || state.had_output)).then_some(state.mute)
        });
        let volume_property = if native_volume { "ao-volume" } else { "volume" };
        let mute_property = if native_mute { "ao-mute" } else { "mute" };
        let mut volume_applied = true;
        let mut mute_applied = true;
        if let Some(volume) = volume {
            // mpv exposes PipeWire's linear amplitude; PulseAudio and desktop
            // mixers use a cubic scale. Keep the same slider loudness on both.
            let volume = if native_volume && driver == "pipewire" {
                100.0 * (volume.min(100.0) / 100.0).powi(3)
            } else if native_volume {
                volume.min(100.0)
            } else {
                volume.min(mpv.get_property::<f64>("volume-max").unwrap_or(150.0))
            };
            if let Err(error) = mpv.set_property(volume_property, volume) {
                volume_applied = false;
                log::warn!("Cannot change playback volume: {error}");
            }
        }
        if let Some(mute) = mute
            && let Err(error) = mpv.set_property(mute_property, mute)
        {
            mute_applied = false;
            log::warn!("Cannot change playback mute: {error}");
        }
        if native_volume && (volume_transition || volume.is_some()) && volume_applied {
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
        if native_mute
            && (mute_transition || mute.is_some())
            && mute_applied
            && let Err(error) = mpv.set_property("mute", false)
        {
            log::warn!("Cannot release software mute: {error}");
        }
        if let Ok(volume) = mpv.get_property::<f64>(volume_property)
            && volume.is_finite()
        {
            state.volume = if native_volume && driver == "pipewire" {
                100.0 * (volume / 100.0).cbrt()
            } else {
                volume
            };
            if native_volume {
                state.volume *= mpv.get_property::<f64>("volume").unwrap_or(100.0) / 100.0;
            }
        }
        if let Ok(mute) = mpv.get_property::<bool>(mute_property) {
            state.mute = mute || (native_mute && mpv.get_property::<bool>("mute").unwrap_or(false));
        }
        let mut shared = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        shared.volume = state.volume;
        shared.mute = state.mute;
        shared.native_volume = native_volume;
        shared.native_mute = native_mute;
        shared.had_output |= !driver.is_empty();
        shared.last_sync = Instant::now();
        Some((state.volume, state.mute))
    }
}
