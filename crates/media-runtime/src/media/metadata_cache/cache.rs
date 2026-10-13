use crate::media::playlist::VideoMeta;
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

pub(in crate::media) fn release_slot(counter: &AtomicU32) {
    counter.fetch_sub(1, Ordering::Release);
    PROBE_CONDVAR.1.notify_one();
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

pub(super) fn save_meta_cache_to_disk(cache: &HashMap<String, CacheEntry>) {
    let Some(path) = meta_cache_path() else {
        return;
    };
    let mut cache_json_root = serde_json::Map::with_capacity(cache.len());
    for (file_path, entry) in cache {
        let mut val = serde_json::Map::new();
        val.insert("mtime".into(), serde_json::Value::from(entry.mtime));
        if let Some(dur) = entry.meta.duration_secs {
            val.insert("duration".into(), serde_json::Value::from(dur));
        }
        if let Some(ref resolution_text) = entry.meta.resolution {
            val.insert(
                "resolution".into(),
                serde_json::Value::from(resolution_text.as_str()),
            );
        }
        if let Some(ref codec) = entry.meta.codec {
            val.insert("codec".into(), serde_json::Value::from(codec.as_str()));
        }
        if entry.meta.audio_tracks > 0 {
            val.insert(
                "audio_tracks".into(),
                serde_json::Value::from(entry.meta.audio_tracks),
            );
        }
        if entry.meta.subtitle_tracks > 0 {
            val.insert(
                "subtitle_tracks".into(),
                serde_json::Value::from(entry.meta.subtitle_tracks),
            );
        }
        cache_json_root.insert(file_path.clone(), serde_json::Value::Object(val));
    }
    let json = serde_json::Value::Object(cache_json_root).to_string();
    if let Err(error) = big_os_kit::storage::atomic_write(&path, json.as_bytes()) {
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
    let Ok(root) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return HashMap::new();
    };
    let Some(cache_json_root) = root.as_object() else {
        return HashMap::new();
    };

    let mut map = HashMap::with_capacity(cache_json_root.len());
    for (file_path, val) in cache_json_root {
        let mtime = val["mtime"].as_u64().unwrap_or(0);
        let entry = CacheEntry {
            meta: VideoMeta {
                duration_secs: val["duration"].as_f64(),
                resolution: val["resolution"].as_str().map(String::from),
                codec: val["codec"].as_str().map(String::from),
                audio_tracks: val["audio_tracks"].as_u64().unwrap_or(0) as u32,
                subtitle_tracks: val["subtitle_tracks"].as_u64().unwrap_or(0) as u32,
            },
            mtime,
        };
        map.insert(file_path.clone(), entry);
    }
    map
}
