use std::sync::atomic::AtomicBool;

use big_media_data::media_probe::probe_json;

use crate::media::playlist::VideoMeta;

use super::cache::{
    ACTIVE_PROBES, CACHE_EVICT_COUNT, CacheEntry, MAX_CACHE_ENTRIES, MAX_PROBES, acquire_slot,
    evict_oldest, file_mtime, meta_cache, release_slot, save_meta_cache_to_disk,
};

/// Synchronous; call from a background thread.
pub fn probe_metadata(path: &str) -> Option<VideoMeta> {
    let root = probe_json(path, &AtomicBool::new(false)).ok()?;
    Some(parse_ffprobe(&root))
}

/// Atomic-semaphore gated to cap concurrent ffprobe children.
pub fn probe_metadata_cached(path: &str) -> Option<VideoMeta> {
    let current_mtime = file_mtime(path);
    if let Some(entry) = meta_cache().lock().ok()?.get(path).cloned()
        && entry.mtime == current_mtime
        && (entry.meta.codec.is_some() || entry.meta.resolution.is_some())
    {
        return Some(entry.meta);
    }

    let fast_dur = super::super::native_duration::fast_read_duration(path);

    acquire_slot(&ACTIVE_PROBES, MAX_PROBES);
    let meta = probe_metadata(path);
    release_slot(&ACTIVE_PROBES);

    let mut meta = meta.unwrap_or_default();
    if meta.duration_secs.is_none() {
        meta.duration_secs = fast_dur;
    }
    if meta.duration_secs.is_none() && meta.codec.is_none() && meta.resolution.is_none() {
        return None;
    }

    if let Ok(mut cache) = meta_cache().lock() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            evict_oldest(&mut cache, CACHE_EVICT_COUNT);
        }
        cache.insert(
            path.to_string(),
            CacheEntry {
                meta: meta.clone(),
                mtime: current_mtime,
            },
        );
        save_meta_cache_to_disk(&cache);
    }
    Some(meta)
}

pub(super) fn parse_ffprobe(root: &serde_json::Value) -> VideoMeta {
    let mut meta = VideoMeta::default();

    if let Some(dur) = root["format"]["duration"].as_str() {
        meta.duration_secs = dur.parse().ok();
    }

    if let Some(streams) = root["streams"].as_array() {
        for stream in streams {
            match stream["codec_type"].as_str() {
                Some("video") => {
                    if meta.resolution.is_none()
                        && let (Some(w), Some(h)) =
                            (stream["width"].as_i64(), stream["height"].as_i64())
                        && w > 0
                        && h > 0
                    {
                        meta.resolution = Some(format!("{w}×{h}"));
                    }
                    if meta.codec.is_none()
                        && let Some(name) = stream["codec_name"].as_str()
                    {
                        meta.codec = Some(codec_display_name(name));
                    }
                }
                Some("audio") => meta.audio_tracks += 1,
                Some("subtitle") => meta.subtitle_tracks += 1,
                _ => {}
            }
        }
    }

    meta
}

pub(super) fn codec_display_name(name: &str) -> String {
    big_media_components::ffmpeg::labels::media_codec_display_name(name)
}
