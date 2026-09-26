//! Hover-thumbnail generator (raw RGB24, 240×135) backed by ffmpeg.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use big_app_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use log::debug;

pub const THUMB_WIDTH: i32 = 240;
pub const THUMB_HEIGHT: i32 = 135;
const THUMB_STRIDE: usize = THUMB_WIDTH as usize * 3; // RGB24

pub struct ThumbnailResult {
    request: ThumbnailRequest,
    pub rgb_data: Arc<[u8]>,
}

/// One pending hover request; decoded pixels are shared by the cache and GTK.
pub struct ThumbnailGenerator {
    sender: async_channel::Sender<Option<ThumbnailRequest>>,
    state: Mutex<ThumbnailState>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ThumbnailRequest {
    generation: u64,
    file: String,
    time_key: i64,
}

#[derive(Default)]
struct ThumbnailState {
    generation: u64,
    requested: Option<ThumbnailRequest>,
}

impl ThumbnailGenerator {
    pub fn new(result_sender: async_channel::Sender<ThumbnailResult>) -> Self {
        let (req_tx, req_rx) = async_channel::bounded::<Option<ThumbnailRequest>>(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        const MAX_CACHE: usize = 8 * 1024 * 1024 / (THUMB_STRIDE * THUMB_HEIGHT as usize);

        std::thread::Builder::new()
            .name("thumbnail-gen".into())
            .spawn(move || {
                let mut cache: HashMap<i64, Arc<[u8]>> = HashMap::new();
                let mut lru_order: VecDeque<i64> = VecDeque::new();
                let mut cached_media = None;

                while let Ok(request) = req_rx.recv_blocking() {
                    if worker_cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let Some(request) = request else {
                        cache.clear();
                        lru_order.clear();
                        cached_media = None;
                        continue;
                    };
                    let media = (request.generation, request.file.clone());
                    if cached_media.as_ref() != Some(&media) {
                        cache.clear();
                        lru_order.clear();
                        cached_media = Some(media);
                    }

                    let pixels = if let Some(pixels) = cache.get(&request.time_key) {
                        pixels.clone()
                    } else {
                        let Some(pixels) = generate_thumbnail(
                            &request.file,
                            request.time_key as f64,
                            &worker_cancelled,
                        ) else {
                            continue;
                        };
                        let pixels: Arc<[u8]> = pixels.into();
                        if cache.len() >= MAX_CACHE
                            && let Some(oldest) = lru_order.pop_front()
                        {
                            cache.remove(&oldest);
                        }
                        cache.insert(request.time_key, pixels.clone());
                        pixels
                    };
                    lru_order.retain(|key| *key != request.time_key);
                    lru_order.push_back(request.time_key);
                    if result_sender
                        .force_send(ThumbnailResult {
                            request,
                            rgb_data: pixels,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("Failed to spawn thumbnail thread");

        Self {
            sender: req_tx,
            state: Mutex::new(ThumbnailState::default()),
            cancelled,
        }
    }

    /// Returns whether the preview must be cleared while a new position loads.
    pub fn request(&self, file: &str, time: f64) -> bool {
        if file.is_empty() || !time.is_finite() || time < 0.0 {
            return false;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let request = ThumbnailRequest {
            generation: state.generation,
            file: file.to_owned(),
            time_key: time.floor() as i64,
        };
        if state.requested.as_ref() == Some(&request) {
            return false;
        }
        state.requested = Some(request.clone());
        let _ = self.sender.force_send(Some(request));
        true
    }

    pub fn is_current(&self, result: &ThumbnailResult) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .requested
            .as_ref()
            == Some(&result.request)
    }

    /// Invalidate even when the same path is reopened after editing the file.
    pub fn invalidate(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.generation = state.generation.wrapping_add(1);
        state.requested = None;
        let _ = self.sender.force_send(None);
    }
}

impl Drop for ThumbnailGenerator {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.sender.close();
    }
}

fn generate_thumbnail(file: &str, time: f64, cancelled: &AtomicBool) -> Option<Vec<u8>> {
    debug!("Generating thumbnail at {time:.3}s");

    let spec = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args([
            "-ss",
            &format!("{time:.3}"),
            "-i",
            file,
            "-frames:v",
            "1",
            "-an",
            "-sn",
            "-vf",
            &format!(
                "scale={THUMB_WIDTH}:{THUMB_HEIGHT}:force_original_aspect_ratio=decrease:reset_sar=1:flags=fast_bilinear,\
                 pad={THUMB_WIDTH}:{THUMB_HEIGHT}:(ow-iw)/2:(oh-ih)/2:black"
            ),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ])
        .stdout(BigSubprocessOutputMode::Capture)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(5))
        .build();
    // run() drains stdout while waiting — no pipe deadlock on the ~97 KB
    // rawvideo (> 64 KB pipe buffer); capture is capped at 16 MB.
    let output = spec.run_cancellable(cancelled).ok()?;
    let expected_size = THUMB_STRIDE * THUMB_HEIGHT as usize;

    if output.status.success() && output.stdout.len() == expected_size {
        Some(output.stdout)
    } else {
        debug!(
            "ffmpeg output unexpected: status={:?} size={} expected={}",
            output.status,
            output.stdout.len(),
            expected_size
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receive(rx: &async_channel::Receiver<ThumbnailResult>) -> ThumbnailResult {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(result) = rx.try_recv() {
                return result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "thumbnail worker produced no result"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn changing_media_does_not_reuse_previous_thumbnail() {
        let dir = tempfile::tempdir().expect("thumbnail fixtures");
        let red = dir.path().join("red.ppm");
        let blue = dir.path().join("blue.ppm");
        std::fs::write(&red, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
        std::fs::write(&blue, b"P6\n1 1\n255\n\x00\x00\xff").unwrap();
        let (tx, rx) = async_channel::bounded(4);
        let generator = ThumbnailGenerator::new(tx);
        generator.request(red.to_str().unwrap(), 0.0);
        let first = receive(&rx);
        generator.request(blue.to_str().unwrap(), 0.0);
        assert!(
            !generator.is_current(&first),
            "reject a result from the previous file"
        );
        let second = receive(&rx);
        let center =
            (THUMB_HEIGHT as usize / 2 * THUMB_WIDTH as usize + THUMB_WIDTH as usize / 2) * 3;
        assert!(
            first.rgb_data[center] > 200,
            "red media must produce red pixels"
        );
        assert!(
            second.rgb_data[center + 2] > 200,
            "changing to blue media at the same timestamp must produce blue pixels"
        );
        assert!(generator.is_current(&second));
        generator.invalidate();
        assert!(
            !generator.is_current(&second),
            "reject results after reopening media"
        );
        std::fs::write(&blue, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
        generator.request(blue.to_str().unwrap(), 0.0);
        let edited = receive(&rx);
        assert!(
            edited.rgb_data[center] > 200,
            "reopening an edited path must invalidate cached pixels"
        );
    }
}
