/// UI thread → cast-session thread.
#[derive(Debug)]
pub enum CastCommand {
    Seek(f64),
    Pause,
    Play,
    /// 0.0–1.0.
    SetVolume(f64),
    /// New URL when transcoding restarts for seek.
    ReloadMedia {
        url: String,
        mime: String,
        seek_secs: f64,
        live: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredCastDevice {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub model: String,
}
