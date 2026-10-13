use big_app_kit::subprocess::BigSubprocessSpec;

use crate::media::playlist::VideoMeta;

use super::cache::{
    ACTIVE_PROBES, CACHE_EVICT_COUNT, CacheEntry, MAX_CACHE_ENTRIES, MAX_PROBES, acquire_slot,
    evict_oldest, file_mtime, meta_cache, release_slot, save_meta_cache_to_disk,
};

/// ffprobe's format and stream report for `path`. Synchronous; call from a
/// background thread.
pub(super) fn ffprobe_json(path: &str) -> Option<serde_json::Value> {
    let output = BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
            // A file name starting with `-` must not read as a flag.
            "--",
            path,
        ])
        .build()
        .run()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).ok()
}

/// Synchronous; call from a background thread.
pub fn probe_metadata(path: &str) -> Option<VideoMeta> {
    ffprobe_json(path).map(|root| parse_ffprobe(&root))
}

/// Cache lookup → native container parser. No ffprobe.
#[must_use]
pub fn fast_duration_cached(path: &str) -> Option<f64> {
    let current_mtime = file_mtime(path);

    if let Some(entry) = meta_cache().lock().ok().and_then(|c| c.get(path).cloned())
        && entry.mtime == current_mtime
    {
        return entry.meta.duration_secs;
    }

    let dur = super::super::native_duration::fast_read_duration(path)?;
    if let Ok(mut cache) = meta_cache().lock() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            evict_oldest(&mut cache, CACHE_EVICT_COUNT);
        }
        cache.entry(path.to_string()).or_insert(CacheEntry {
            meta: VideoMeta {
                duration_secs: Some(dur),
                ..Default::default()
            },
            mtime: current_mtime,
        });
    }
    Some(dur)
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
