use std::sync::{Arc, Mutex, atomic::AtomicBool};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaybackStatus { Playing, Paused, #[default] Stopped }
impl PlaybackStatus {
    pub fn as_str(self) -> &'static str { match self { Self::Playing => "Playing", Self::Paused => "Paused", Self::Stopped => "Stopped" } }
}
/// Typed intentions. Track identity survives the bus -> GTK mailbox crossing.
#[derive(Debug, Clone, PartialEq)]
pub enum MprisCommand {
    Raise, Quit, Next, Previous, Play, Pause, PlayPause, Stop,
    Seek { track_id: String, offset_us: i64 },
    SetPosition { track_id: String, position_us: i64 },
    OpenUri(String), SetVolume(f64), SetRate(f64), SetShuffle(bool), SetLoopStatus(String),
}
pub type CommandCallback = Box<dyn Fn(MprisCommand) + Send + Sync + 'static>;
pub type StateCallback = Box<dyn Fn() -> PlayerState + Send + Sync + 'static>;
/// One coherent snapshot, published only after processing a player/queue event.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerState {
    pub status: PlaybackStatus,
    pub track_id: Option<String>,
    pub position_secs: f64, pub duration_secs: f64, pub volume_pct: f64, pub rate: f64,
    pub title: String, pub artist: String, pub album: String, pub art_url: String,
    pub can_next: bool, pub can_prev: bool, pub can_play: bool, pub can_pause: bool, pub can_seek: bool,
    pub loop_status: String, pub shuffle: bool,
}
pub(super) fn micros(value: f64) -> i64 {
    if value.is_finite() && value > 0.0 { (value * 1_000_000.0).min(i64::MAX as f64).round() as i64 } else { 0 }
}
pub(super) fn volume(value: f64) -> f64 { if value.is_finite() { value.max(0.0) / 100.0 } else { 0.0 } }
pub(super) fn rate(value: f64) -> f64 { if value.is_finite() && value > 0.0 { value.clamp(0.25, 4.0) } else { 1.0 } }
pub(super) fn previous_state() -> Arc<Mutex<Option<PlayerState>>> { Arc::new(Mutex::new(None)) }
pub(super) fn taskbar_flag(value: bool) -> Arc<AtomicBool> { Arc::new(AtomicBool::new(value)) }
