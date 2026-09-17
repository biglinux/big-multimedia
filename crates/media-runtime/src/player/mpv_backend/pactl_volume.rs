//! Compatibility bridge for per-stream PulseAudio / pipewire-pulse volume.
//!
//! One worker per mpv instance serialises and coalesces writes. UI access is a
//! cached snapshot, never a subprocess. Polling stops without a loaded file.
//! This is a bounded compatibility path, not a native sound-server connection;
//! a subscription-based native backend remains the preferred follow-up.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use serde::Deserialize;

#[derive(Default)]
struct State {
    active: bool,
    dirty: bool,
    desired_volume: Option<f64>,
    desired_mute: Option<bool>,
    observed: Option<(f64, bool)>,
}

#[derive(Clone)]
pub(super) struct Shared(Arc<(Mutex<State>, Condvar)>);

impl Shared {
    pub(super) fn set_active(&self, active: bool) {
        let mut state = self.0.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.active = active;
        state.dirty = true;
        self.0.1.notify_one();
    }

    pub(super) fn snapshot(&self) -> Option<(f64, bool)> {
        let state = self.0.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.observed.is_none()
            && state.desired_volume.is_none()
            && state.desired_mute.is_none()
        {
            return None;
        }
        let (volume, mute) = state.observed.unwrap_or((100.0, false));
        Some((
            state.desired_volume.unwrap_or(volume),
            state.desired_mute.unwrap_or(mute),
        ))
    }
}

pub(super) struct StreamVolume {
    shared: Shared,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl StreamVolume {
    pub(super) fn new(client_name: String) -> Self {
        let shared = Shared(Arc::new((Mutex::new(State::default()), Condvar::new())));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_shared = shared.clone();
        let worker_cancel = Arc::clone(&cancelled);
        let worker = std::thread::Builder::new()
            .name("player-stream-volume".into())
            .spawn(move || run_worker(&worker_shared, &worker_cancel, &client_name))
            .map_err(|error| log::error!("Cannot start stream-volume worker: {error}"))
            .ok();
        Self {
            shared,
            cancelled,
            worker,
        }
    }

    pub(super) fn shared(&self) -> Shared {
        self.shared.clone()
    }

    pub(super) fn snapshot(&self) -> (f64, bool) {
        self.shared.snapshot().unwrap_or((100.0, false))
    }

    pub(super) fn set_volume(&self, percent: f64) {
        if !percent.is_finite() {
            return;
        }
        let mut state = self
            .shared
            .0
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.desired_volume = Some(percent.clamp(0.0, 250.0).round());
        state.dirty = true;
        self.shared.0.1.notify_one();
    }

    pub(super) fn set_mute(&self, mute: bool) {
        let mut state = self
            .shared
            .0
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.desired_mute = Some(mute);
        state.dirty = true;
        self.shared.0.1.notify_one();
    }
}

impl Drop for StreamVolume {
    fn drop(&mut self) {
        // Pair cancellation with the condition-variable mutex to avoid losing
        // the notification between the worker's predicate check and wait.
        {
            let _state = self
                .shared
                .0
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            self.cancelled.store(true, Ordering::Relaxed);
            self.shared.0.1.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Deserialize)]
struct SinkInput {
    index: u32,
    #[serde(default)]
    mute: bool,
    #[serde(default)]
    properties: Properties,
    #[serde(default)]
    volume: std::collections::HashMap<String, VolumeChannel>,
}

#[derive(Deserialize, Default)]
struct Properties {
    #[serde(rename = "application.name", default)]
    application_name: String,
    #[serde(rename = "application.process.id", default)]
    process_id: String,
}

#[derive(Deserialize)]
struct VolumeChannel {
    value_percent: String,
}

fn parse_percent(value: &str) -> Option<f64> {
    let value: f64 = value.trim().trim_end_matches('%').trim().parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn pactl(args: &[&str], cancel: &AtomicBool) -> Option<Vec<u8>> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let output = BigSubprocessSpec::builder()
        .program("pactl")
        .args(args.iter().copied())
        .env("LC_ALL", "C")
        .timeout(Duration::from_secs(2))
        .stdout(BigSubprocessOutputMode::Capture)
        .stderr(BigSubprocessOutputMode::Null)
        .build()
        .run_cancellable(cancel)
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// Never fall back to another process with the same application name, and do
/// not guess when multiple mpv streams in this process are indistinguishable.
fn select_input(inputs: Vec<SinkInput>, client_name: &str, pid: &str) -> Option<SinkInput> {
    let mut matches = inputs.into_iter().filter(|input| {
        input.properties.application_name == client_name && input.properties.process_id == pid
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn find(client_name: &str, cancel: &AtomicBool) -> Option<SinkInput> {
    let out = pactl(&["-f", "json", "list", "sink-inputs"], cancel)?;
    select_input(
        serde_json::from_slice(&out).ok()?,
        client_name,
        &std::process::id().to_string(),
    )
}

fn input_volume(input: &SinkInput) -> Option<f64> {
    // Deterministic for unequal channel gains; HashMap iteration order must not
    // make a single slider jump randomly between left and right channel levels.
    input
        .volume
        .values()
        .filter_map(|channel| parse_percent(&channel.value_percent))
        .reduce(f64::max)
}

fn run_worker(shared: &Shared, cancel: &AtomicBool, client_name: &str) {
    let mut next_poll = Instant::now();
    let mut missing_delay = Duration::from_millis(250);
    loop {
        let (volume, mute) = {
            let mut state = shared.0.0.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                if state.active && (state.dirty || Instant::now() >= next_poll) {
                    break;
                }
                state = if state.active {
                    shared
                        .0
                        .1
                        .wait_timeout(state, next_poll.saturating_duration_since(Instant::now()))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                } else {
                    shared
                        .0
                        .1
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner)
                };
            }
            state.dirty = false;
            (state.desired_volume, state.desired_mute)
        };
        // Do not retain an index across ticks: mpv can replace its sink-input,
        // and a stale index must not direct a later write at another stream.
        if let Some(input) = find(client_name, cancel) {
            let id = input.index.to_string();
            let volume_ok = volume.is_some_and(|value| {
                pactl(
                    &["set-sink-input-volume", &id, &format!("{value}%")],
                    cancel,
                )
                .is_some()
            });
            let mute_ok = mute.is_some_and(|value| {
                pactl(
                    &["set-sink-input-mute", &id, if value { "1" } else { "0" }],
                    cancel,
                )
                .is_some()
            });
            let mut state = shared.0.0.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(observed_volume) = input_volume(&input) {
                state.observed = Some((
                    if volume_ok {
                        volume.unwrap_or(observed_volume)
                    } else {
                        observed_volume
                    },
                    if mute_ok {
                        mute.unwrap_or(input.mute)
                    } else {
                        input.mute
                    },
                ));
            }
            // Clear only the version just applied; edits arriving during I/O
            // remain pending and will be the next operation, never reordered.
            if volume_ok && state.desired_volume == volume {
                state.desired_volume = None;
            }
            if mute_ok && state.desired_mute == mute {
                state.desired_mute = None;
            }
            missing_delay = Duration::from_millis(250);
            next_poll = Instant::now() + Duration::from_secs(2);
        } else {
            // FileLoaded may precede audio stream creation. Retain requested
            // values and retry with bounded backoff rather than dropping them.
            next_poll = Instant::now() + missing_delay;
            missing_delay = (missing_delay * 2).min(Duration::from_secs(8));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_are_finite_and_nonnegative() {
        assert_eq!(parse_percent(" 130% "), Some(130.0));
        assert_eq!(parse_percent("0%"), Some(0.0));
        assert_eq!(parse_percent("42"), Some(42.0));
        for invalid in ["", "%", "NaN%", "inf", "-1%", "loud"] {
            assert_eq!(parse_percent(invalid), None);
        }
    }

    fn stream(index: u32, pid: &str) -> SinkInput {
        SinkInput {
            index,
            mute: false,
            properties: Properties {
                application_name: "player".into(),
                process_id: pid.into(),
            },
            volume: std::collections::HashMap::default(),
        }
    }

    #[test]
    fn never_controls_a_different_process_or_ambiguous_stream() {
        assert!(select_input(vec![stream(1, "other")], "player", "ours").is_none());
        assert!(
            select_input(vec![stream(1, "ours"), stream(2, "ours")], "player", "ours").is_none()
        );
        assert_eq!(
            select_input(
                vec![stream(1, "other"), stream(2, "ours")],
                "player",
                "ours"
            )
            .unwrap()
            .index,
            2
        );
    }

    #[test]
    fn latest_desired_value_wins_without_io() {
        let volume = StreamVolume {
            shared: Shared(Arc::new((Mutex::new(State::default()), Condvar::new()))),
            cancelled: Arc::new(AtomicBool::new(false)),
            worker: None,
        };
        for value in 0..1000 {
            volume.set_volume(f64::from(value));
        }
        volume.set_volume(42.0);
        volume.set_volume(f64::NAN);
        volume.set_mute(true);
        assert_eq!(volume.snapshot(), (42.0, true));
    }
}
