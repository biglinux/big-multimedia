//! Hover-thumbnail generator (raw RGB24, 240×135) backed by ffmpeg.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use big_app_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use log::debug;

pub const THUMB_WIDTH: i32 = 240;
pub const THUMB_HEIGHT: i32 = 135;
const THUMB_STRIDE: usize = THUMB_WIDTH as usize * 3; // RGB24

pub struct ThumbnailResult {
    pub time_key: i64,
    pub rgb_data: Vec<u8>,
}

/// Raw RGB24 → MemoryTexture, no JPEG round-trip.
/// Requests coalesce to the latest hover position. ffmpeg children are NOT
/// cancelled — each completes in ~0.2s, yielding ~5fps updates during drag.
pub struct ThumbnailGenerator {
    sender: std::sync::mpsc::Sender<ThumbnailRequest>,
    running: Arc<AtomicBool>,
}

struct ThumbnailRequest {
    time_key: i64,
    actual_time: f64,
}

impl ThumbnailGenerator {
    pub fn new(
        result_sender: async_channel::Sender<ThumbnailResult>,
        file_path: Arc<Mutex<Option<String>>>,
    ) -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::channel::<ThumbnailRequest>();
        let running = Arc::new(AtomicBool::new(true));
        let run_flag = running.clone();
        const MAX_CACHE: usize = 200;

        std::thread::Builder::new()
            .name("thumbnail-gen".into())
            .spawn(move || {
                let mut cache: HashMap<i64, Vec<u8>> = HashMap::new();
                let mut lru_order: VecDeque<i64> = VecDeque::new();

                while run_flag.load(Ordering::Relaxed) {
                    let Ok(first) = req_rx.recv() else { break };

                    // Coalesce — drop intermediate hover positions.
                    let mut latest = first;
                    while let Ok(req) = req_rx.try_recv() {
                        latest = req;
                    }

                    let file = {
                        let guard = file_path
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        match guard.as_ref() {
                            Some(f) => f.clone(),
                            None => continue,
                        }
                    };

                    if let Some(cached_rgb_pixels) = cache.get(&latest.time_key) {
                        let cached_rgb_pixels = cached_rgb_pixels.clone();
                        // LRU bump.
                        if let Some(pos) = lru_order.iter().position(|k| *k == latest.time_key) {
                            lru_order.remove(pos);
                        }
                        lru_order.push_back(latest.time_key);
                        let _ = result_sender.send_blocking(ThumbnailResult {
                            time_key: latest.time_key,
                            rgb_data: cached_rgb_pixels,
                        });
                        continue;
                    }

                    if let Some(generated_rgb_pixels) =
                        Self::generate_ffmpeg(&file, latest.actual_time)
                    {
                        if cache.len() >= MAX_CACHE
                            && let Some(oldest) = lru_order.pop_front()
                        {
                            cache.remove(&oldest);
                        }
                        cache.insert(latest.time_key, generated_rgb_pixels.clone());
                        lru_order.push_back(latest.time_key);
                        let _ = result_sender.send_blocking(ThumbnailResult {
                            time_key: latest.time_key,
                            rgb_data: generated_rgb_pixels,
                        });
                    }
                }
            })
            .expect("Failed to spawn thumbnail thread");

        Self {
            sender: req_tx,
            running,
        }
    }

    /// Time is rounded to 1-second intervals so cache hits cluster.
    pub fn request(&self, _file: &str, time: f64) {
        let time_key = time.round() as i64;
        let _ = self.sender.send(ThumbnailRequest {
            time_key,
            actual_time: time,
        });
    }

    fn generate_ffmpeg(file: &str, time: f64) -> Option<Vec<u8>> {
        generate_rgb24_thumbnail(file, time)
    }
}

impl Drop for ThumbnailGenerator {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// Generate a padded RGB24 thumbnail at `time`.
///
/// Blocking. Call only from a worker thread.
#[must_use]
pub fn generate_rgb24_thumbnail(file: &str, time: f64) -> Option<Vec<u8>> {
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
                "scale={THUMB_WIDTH}:{THUMB_HEIGHT}:force_original_aspect_ratio=decrease:flags=fast_bilinear,\
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
        .build();
    // run() drains stdout while waiting — no pipe deadlock on the ~97 KB
    // rawvideo (> 64 KB pipe buffer); capture is capped at 16 MB.
    let output = spec.run().ok()?;
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
