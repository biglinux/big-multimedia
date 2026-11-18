use crate::media::playlist::VideoMeta;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Condvar, LazyLock, Mutex, OnceLock};

pub(super) const MAX_PROBES: u32 = 4;
pub(super) const MAX_CACHE_ENTRIES: usize = 2000;
pub(super) const CACHE_EVICT_COUNT: usize = 500;

pub(super) static ACTIVE_PROBES: AtomicU32 = AtomicU32::new(0);
static PROBE_CONDVAR: LazyLock<(Mutex<()>, Condvar)> =
    LazyLock::new(|| (Mutex::new(()), Condvar::new()));

/// `mtime` invalidates the entry when the file changes on disk.
#[derive(Clone)]
pub(in crate::media) struct CacheEntry {
    pub(in crate::media) meta: VideoMeta,
    pub(in crate::media) mtime: u64,
}

/// Blocks via CondVar until a slot is available.
pub(in crate::media) fn acquire_slot(counter: &AtomicU32, max: u32) {
    let (lock, cvar) = &*PROBE_CONDVAR;
    let mut guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    loop {
        let current = counter.load(Ordering::Acquire);
        if current < max
            && counter
                .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            return;
        }
        guard = cvar
            .wait(guard)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}

/// Under the lock a waiter checks with, so the wake-up cannot fall between
/// its check and its wait; every waiter wakes, as probes and thumbnails
/// share the condvar and one of the other kind would take the only wake-up.
pub(in crate::media) fn release_slot(counter: &AtomicU32) {
    let (lock, cvar) = &*PROBE_CONDVAR;
    let _guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    counter.fetch_sub(1, Ordering::Release);
    cvar.notify_all();
}

/// Persisted to disk so metadata survives restarts.
pub(in crate::media) fn meta_cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(load_meta_cache_from_disk()))
}

pub(super) fn file_mtime(path: &str) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map_or(0, |t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
}

/// One file's entry as the cache file stores it; a missing field reads as
/// unknown.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredEntry {
    mtime: u64,
    duration: Option<f64>,
    resolution: Option<String>,
    codec: Option<String>,
    audio_tracks: u32,
    subtitle_tracks: u32,
}

pub(super) fn save_meta_cache_to_disk(cache: &HashMap<String, CacheEntry>) {
    let Some(path) = meta_cache_path() else {
        return;
    };
    let stored: HashMap<&str, StoredEntry> = cache
        .iter()
        .map(|(file_path, entry)| {
            let stored = StoredEntry {
                mtime: entry.mtime,
                duration: entry.meta.duration_secs,
                resolution: entry.meta.resolution.clone(),
                codec: entry.meta.codec.clone(),
                audio_tracks: entry.meta.audio_tracks,
                subtitle_tracks: entry.meta.subtitle_tracks,
            };
            (file_path.as_str(), stored)
        })
        .collect();
    let json = match serde_json::to_vec(&stored) {
        Ok(json) => json,
        Err(error) => {
            log::debug!("metadata cache not saved: {error}");
            return;
        }
    };
    if let Err(error) = big_os_kit::storage::atomic_write(&path, &json) {
        log::debug!("metadata cache not saved: {error}");
    }
}

/// Evicts the `count` oldest entries by mtime.
pub(super) fn evict_oldest(cache: &mut HashMap<String, CacheEntry>, count: usize) {
    if cache.len() <= count {
        cache.clear();
        return;
    }

    // O(n) partition instead of O(n log n) full sort.
    let mut entries: Vec<(String, u64)> = cache.iter().map(|(k, v)| (k.clone(), v.mtime)).collect();
    entries.select_nth_unstable_by_key(count, |&(_, mtime)| mtime);
    for (key, _) in entries.into_iter().take(count) {
        cache.remove(&key);
    }
}

/// `None` without a private cache directory: the cache then lives in memory only.
fn meta_cache_path() -> Option<PathBuf> {
    Some(
        big_os_kit::xdg_dirs::cache_dir()?
            .join(crate::config::video::CONFIG_DIR)
            .join("metadata_cache.json"),
    )
}

/// Far above what `MAX_CACHE_ENTRIES` entries take.
const MAX_CACHE_FILE_BYTES: usize = 16 * 1024 * 1024;

fn load_meta_cache_from_disk() -> HashMap<String, CacheEntry> {
    let Some(bytes) = meta_cache_path()
        .and_then(|path| {
            big_os_kit::storage::read_regular_limited(&path, MAX_CACHE_FILE_BYTES).ok()
        })
        .flatten()
    else {
        return HashMap::new();
    };
    parse_meta_cache(&bytes)
}

pub(super) fn parse_meta_cache(bytes: &[u8]) -> HashMap<String, CacheEntry> {
    let Ok(root) = serde_json::from_slice::<HashMap<String, serde_json::Value>>(bytes) else {
        return HashMap::new();
    };

    // An entry that does not read is dropped alone; the rest stay cached.
    let mut map = HashMap::with_capacity(root.len());
    for (file_path, value) in root {
        let Ok(stored) = serde_json::from_value::<StoredEntry>(value) else {
            continue;
        };
        let entry = CacheEntry {
            meta: VideoMeta {
                duration_secs: stored.duration,
                resolution: stored.resolution,
                codec: stored.codec,
                audio_tracks: stored.audio_tracks,
                subtitle_tracks: stored.subtitle_tracks,
            },
            mtime: stored.mtime,
        };
        map.insert(file_path, entry);
    }
    map
}
