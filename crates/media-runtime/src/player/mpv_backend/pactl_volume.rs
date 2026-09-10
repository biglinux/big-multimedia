//! Direct control of THIS process's PipeWire/PulseAudio playback stream volume,
//! so the in-app volume *is* the sink-input volume the system mixer shows — the
//! same percentage, and with >100% boost. mpv's own controls can't do this:
//! software `volume` never reaches the sound server (the mixer stays at 100%),
//! and `ao-volume` is capped at 100 and uses a different (linear-amplitude)
//! scale than the mixer's perceptual one. Best-effort via `pactl` (present with
//! PulseAudio or pipewire-pulse); every function no-ops when it is missing or
//! nothing is playing, in which case the sound server restores the last level
//! for our stable client name (`module-stream-restore`).
//!
//! Writes (`set_volume`/`set_mute`) run OFF the GTK main loop on a detached
//! thread — a `pactl` round-trip is a few milliseconds and the volume slider
//! fires many changes per drag. A per-operation generation counter coalesces a
//! burst to its latest value: an older write sees a newer generation and skips,
//! so the last drag position wins without blocking or queuing.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use serde::Deserialize;

static VOLUME_GEN: AtomicU64 = AtomicU64::new(0);
static MUTE_GEN: AtomicU64 = AtomicU64::new(0);

type Cache = Arc<Mutex<Option<u32>>>;

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
    /// pactl reports e.g. `"55%"` (perceptual, the value the mixer shows).
    value_percent: String,
}

/// Parse pactl's `"55%"` channel value into a plain percentage.
fn parse_percent(value: &str) -> Option<f64> {
    value.trim().trim_end_matches('%').trim().parse().ok()
}

fn pactl(args: &[&str]) -> Option<std::process::Output> {
    Command::new("pactl")
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
}

/// Our own playback stream, matched by the `application.name` mpv reports (the
/// `audio-client-name`, i.e. the program name). When several streams share the
/// name (a second instance of the same player), prefer the one whose
/// `application.process.id` is ours so each instance controls its own stream.
fn find(client_name: &str) -> Option<SinkInput> {
    let out = pactl(&["-f", "json", "list", "sink-inputs"])?;
    let inputs: Vec<SinkInput> = serde_json::from_slice(&out.stdout).ok()?;
    let mut ours: Vec<SinkInput> = inputs
        .into_iter()
        .filter(|input| input.properties.application_name == client_name)
        .collect();
    if ours.is_empty() {
        return None;
    }
    let pid = std::process::id().to_string();
    let idx = ours
        .iter()
        .position(|input| input.properties.process_id == pid)
        .unwrap_or(0);
    Some(ours.swap_remove(idx))
}

fn cached_id(cache: &Cache, client_name: &str) -> Option<u32> {
    let mut guard = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.is_none() {
        *guard = find(client_name).map(|input| input.index);
    }
    *guard
}

fn invalidate(cache: &Cache) {
    *cache.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// Apply a `set-sink-input-*` verb to our stream. mpv recreates the stream (new
/// index) per file, so a stale cached index is dropped and re-resolved once.
fn apply(cache: &Cache, client_name: &str, verb: &str, value: &str) {
    for _ in 0..2 {
        let Some(id) = cached_id(cache, client_name) else {
            return;
        };
        if pactl(&[verb, &id.to_string(), value]).is_some() {
            return;
        }
        invalidate(cache);
    }
}

/// Set our stream to `percent` on the mixer's scale (100 = 0 dB, up to ~150 for
/// boost). The sound server remembers it per client name across files/sessions.
/// Applied on a detached thread; a newer call supersedes this one.
pub fn set_volume(cache: &Cache, client_name: &str, percent: f64) {
    let generation = VOLUME_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    let value = format!("{}%", percent.clamp(0.0, 250.0).round() as i64);
    let cache = Arc::clone(cache);
    let client_name = client_name.to_string();
    thread::spawn(move || {
        if VOLUME_GEN.load(Ordering::Relaxed) == generation {
            apply(&cache, &client_name, "set-sink-input-volume", &value);
        }
    });
}

pub fn set_mute(cache: &Cache, client_name: &str, mute: bool) {
    let generation = MUTE_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    let value = if mute { "1" } else { "0" };
    let cache = Arc::clone(cache);
    let client_name = client_name.to_string();
    thread::spawn(move || {
        if MUTE_GEN.load(Ordering::Relaxed) == generation {
            apply(&cache, &client_name, "set-sink-input-mute", value);
        }
    });
}

/// Current `(volume percent, muted)` for our stream in a single `pactl` query,
/// or `None` while nothing plays. Used by the mixer-mirroring poll.
pub fn get_state(cache: &Cache, client_name: &str) -> Option<(f64, bool)> {
    let input = find(client_name)?;
    *cache.lock().unwrap_or_else(PoisonError::into_inner) = Some(input.index);
    let volume = parse_percent(&input.volume.values().next()?.value_percent)?;
    Some((volume, input.mute))
}

/// Current stream volume on the mixer's scale, or `None` while nothing plays.
pub fn get_volume(cache: &Cache, client_name: &str) -> Option<f64> {
    get_state(cache, client_name).map(|(volume, _)| volume)
}

pub fn get_mute(client_name: &str) -> Option<bool> {
    Some(find(client_name)?.mute)
}

#[cfg(test)]
mod tests {
    use super::parse_percent;

    #[test]
    fn parse_percent_reads_plain_and_padded() {
        assert_eq!(parse_percent("55%"), Some(55.0));
        assert_eq!(parse_percent(" 130% "), Some(130.0));
        assert_eq!(parse_percent("0%"), Some(0.0));
    }

    #[test]
    fn parse_percent_rejects_garbage() {
        assert_eq!(parse_percent(""), None);
        assert_eq!(parse_percent("%"), None);
        assert_eq!(parse_percent("loud"), None);
    }

    #[test]
    fn parse_percent_handles_missing_suffix() {
        assert_eq!(parse_percent("42"), Some(42.0));
    }
}
