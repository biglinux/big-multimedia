use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use super::DiscoveredCastDevice;

pub struct CastSessionController {
    pub(super) devices: Arc<Mutex<Vec<DiscoveredCastDevice>>>,
    pub(super) casting: Arc<AtomicBool>,
    /// Heartbeat-thread stop signal.
    pub(super) stop_cast: Arc<AtomicBool>,
    pub(super) stop_http: Arc<AtomicBool>,
    pub(super) discovering: Arc<AtomicBool>,
    pub(super) cast_thread: Mutex<Option<thread::JoinHandle<()>>>,
    pub(super) cmd_tx: Mutex<Option<std::sync::mpsc::Sender<super::CastCommand>>>,
    pub(super) casting_device_name: Mutex<Option<String>>,
    /// Transcoding (re-encode) vs direct streaming.
    pub(super) transcoding: Arc<AtomicBool>,
    /// Used to restart FFmpeg on seek.
    pub(super) transcode_session: Arc<Mutex<Option<crate::cast::transcode::TranscodeSession>>>,
    pub(super) local_ip: Mutex<Option<String>>,
    pub(super) cast_file_path: Mutex<Option<String>>,
    /// Pre-fills filter dialogs with the active values.
    pub(super) active_filters: Mutex<crate::cast::transcode::TranscodeFilters>,
    /// Encoder detected for this cast session. Detection runs lspci plus an
    /// ffmpeg probe per device (5 s deadline each), so seeks and filter
    /// changes reuse it; `stop_casting` clears it so the next cast re-detects.
    pub(super) detected_gpu: Mutex<Option<(crate::cast::gpu::GpuType, String)>>,
}

impl Default for CastSessionController {
    fn default() -> Self {
        Self::new()
    }
}

impl CastSessionController {
    #[must_use]
    pub fn new() -> Self {
        Self {
            devices: Arc::new(Mutex::new(Vec::new())),
            casting: Arc::new(AtomicBool::new(false)),
            stop_cast: Arc::new(AtomicBool::new(false)),
            stop_http: Arc::new(AtomicBool::new(false)),
            discovering: Arc::new(AtomicBool::new(false)),
            cast_thread: Mutex::new(None),
            cmd_tx: Mutex::new(None),
            casting_device_name: Mutex::new(None),
            transcoding: Arc::new(AtomicBool::new(false)),
            transcode_session: Arc::new(Mutex::new(None)),
            local_ip: Mutex::new(None),
            cast_file_path: Mutex::new(None),
            active_filters: Mutex::new(crate::cast::transcode::TranscodeFilters::default()),
            detected_gpu: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn get_devices(&self) -> Vec<DiscoveredCastDevice> {
        self.devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn clear_devices(&self) {
        self.devices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    #[must_use]
    pub fn is_discovering(&self) -> bool {
        self.discovering.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn is_casting(&self) -> bool {
        self.casting.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn is_transcoding(&self) -> bool {
        self.transcoding.load(Ordering::Relaxed)
    }

    pub fn casting_device_name(&self) -> Option<String> {
        self.casting_device_name
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Encoder for a transcode: the user's override, else this session's
    /// detected device. Call from a worker; the first detection can block.
    pub(super) fn resolve_gpu(
        &self,
        filters: &crate::cast::transcode::TranscodeFilters,
    ) -> (crate::cast::gpu::GpuType, String) {
        if let Some(gpu) = filters.gpu_override {
            return (gpu, filters.gpu_device_path.clone());
        }
        self.detected_gpu
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_or_insert_with(crate::cast::gpu::detect_gpu_device)
            .clone()
    }

    pub fn active_filters(&self) -> crate::cast::transcode::TranscodeFilters {
        self.active_filters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(super) fn set_cast_state(&self, device_name: Option<String>, transcoding: bool) {
        *self
            .casting_device_name
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = device_name;
        self.transcoding.store(transcoding, Ordering::Relaxed);
    }
}
