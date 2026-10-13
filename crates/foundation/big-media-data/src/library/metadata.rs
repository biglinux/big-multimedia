use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessOutputMode, BigSubprocessSpec};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::db::models::Track;
use crate::error::{Error, Result};

pub fn read_track_metadata(path: &str) -> Result<Track> {
    read_track_metadata_cancellable(path, &AtomicBool::new(false))
}
/// Worker-only metadata probe bounded by the shared process supervisor.
pub fn read_track_metadata_cancellable(path: &str, cancelled: &AtomicBool) -> Result<Track> {
    if cancelled.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let file_path = Path::new(path);
    let probe = ffprobe_json_cancellable(path, cancelled)?;

    let file_meta = fs::metadata(file_path).ok();
    let file_size = file_meta.as_ref().map(|m| m.len() as i64);

    let format_str = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_uppercase);

    let audio = first_audio_stream(&probe);
    let duration_secs = probe_number(&probe["format"]["duration"])
        .or_else(|| audio.and_then(|stream| probe_number(&stream["duration"])))
        .unwrap_or(0.0);
    let bitrate = audio
        .and_then(|stream| probe_i32(&stream["bit_rate"]))
        .or_else(|| probe_i32(&probe["format"]["bit_rate"]));
    let sample_rate = audio.and_then(|stream| probe_i32(&stream["sample_rate"]));
    let channels = audio.and_then(|stream| probe_i32(&stream["channels"]));

    let title = tag_value(&probe, &["title"]);
    let artist = tag_value(&probe, &["artist", "album_artist"]);
    let album = tag_value(&probe, &["album"]);
    let album_artist = tag_value(&probe, &["album_artist", "albumartist"]);
    let track_number = tag_value(&probe, &["track", "tracknumber"]).and_then(|s| parse_tag_i32(&s));
    let disc_number = tag_value(&probe, &["disc", "discnumber"]).and_then(|s| parse_tag_i32(&s));
    let year = tag_value(&probe, &["date", "year"]).and_then(|s| parse_year(&s));
    let genre = tag_value(&probe, &["genre"]);
    // Embedded art is a video stream; without one, asking ffmpeg for it only
    // costs a second process per file.
    let has_picture_stream = probe["streams"]
        .as_array()
        .is_some_and(|streams| streams.iter().any(|stream| stream["codec_type"] == "video"));
    let art_hash = album_art(path, cancelled, has_picture_stream).map(|data| {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    });

    let modified_at = file_meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    if cancelled.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if [&title, &artist, &album, &album_artist, &genre]
        .iter()
        .any(|v| v.as_ref().is_some_and(|v| v.len() > 4096))
    {
        return Err(Error::invalid_argument(
            "metadata",
            "Media text fields exceed their limit",
        ));
    }
    Ok(Track {
        id: 0,
        file_path: path.to_string(),
        title,
        artist,
        album,
        album_artist,
        track_number,
        disc_number,
        year,
        genre,
        duration_secs,
        file_size,
        bitrate,
        sample_rate,
        channels,
        format: format_str,
        art_hash,
        date_added: 0,
        last_played: None,
        play_count: 0,
        rating: 0,
        modified_at,
    })
}

#[derive(Debug, Clone, Default)]
pub struct TagEdits {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<String>,
    pub genre: Option<String>,
}

/// Stream copy of a large file on slow storage; still bounded.
const TAG_REWRITE_TIMEOUT: Duration = Duration::from_secs(600);

/// Blocking; rewrites the file through ffmpeg and renames it into place.
pub fn write_track_tags(path: &str, edits: &TagEdits) -> Result<()> {
    let source = Path::new(path);
    let parent = source
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let suffix = source
        .extension()
        .and_then(|ext| ext.to_str())
        .map_or_else(String::new, |ext| format!(".{ext}"));
    // ffmpeg overwrites this exclusively created file (`-y`); it stays ours
    // until the rename, so nothing can be planted at its name meanwhile.
    let temp = tempfile::Builder::new()
        .prefix(".big-media-tags-")
        .suffix(&suffix)
        .tempfile_in(parent)
        .map_err(|e| Error::internal(format!("cannot create tag temp file: {e}")))?;
    let output = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args(["-map", "0", "-c", "copy", "-map_metadata", "0"])
        .args(metadata_args(edits))
        .arg(temp.path())
        .stdout(BigSubprocessOutputMode::Null)
        .timeout(TAG_REWRITE_TIMEOUT)
        .build()
        .run()
        .map_err(|e| Error::internal(format!("ffmpeg tag rewrite failed: {e}")))?;
    if !output.status.success() {
        return Err(Error::internal(format!(
            "ffmpeg tag rewrite failed: {}",
            output.stderr_lossy()
        )));
    }
    if let Ok(meta) = fs::metadata(source) {
        let _ = temp.as_file().set_permissions(meta.permissions());
    }
    // The rewrite replaces the user's only copy of the file: its data must be
    // on disk before the rename can expose it.
    temp.as_file().sync_all()?;
    temp.persist(source)
        .map_err(|e| Error::internal(format!("failed to promote rewritten tags: {}", e.error)))?;
    if let Err(error) = big_os_kit::storage::sync_parent(source) {
        log::warn!("tag rewrite renamed but not synced: {error}");
    }
    Ok(())
}

/// Embedded picture first; falls back to sibling Cover/folder/front/album/
/// albumart/thumb × jpg/jpeg/png/webp.
pub fn extract_album_art(path: &str) -> Option<Vec<u8>> {
    album_art(path, &AtomicBool::new(false), true)
}
fn album_art(path: &str, cancelled: &AtomicBool, try_embedded: bool) -> Option<Vec<u8>> {
    if cancelled.load(Ordering::Acquire) {
        return None;
    }
    if try_embedded && let Some(data) = extract_embedded_album_art(path, cancelled) {
        return Some(data);
    }

    let dir = std::path::Path::new(path).parent()?;
    const COVER_NAMES: &[&str] = &[
        "cover", "Cover", "folder", "Folder", "front", "Front", "album", "Album", "albumart",
        "AlbumArt", "thumb", "Thumb",
    ];
    const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
    for name in COVER_NAMES {
        for ext in EXTENSIONS {
            let candidate = dir.join(format!("{name}.{ext}"));
            if cancelled.load(Ordering::Acquire) {
                return None;
            }
            if let Ok(Some(data)) =
                big_os_kit::storage::read_regular_limited(&candidate, 16 * 1024 * 1024)
                && !data.is_empty()
            {
                return Some(data);
            }
        }
    }
    None
}

/// External `.lrc` sidecar wins over embedded lyrics when both exist.
pub fn extract_lyrics(path: &str) -> Option<String> {
    let lrc_path = Path::new(path).with_extension("lrc");
    if let Ok(Some(bytes)) = big_os_kit::storage::read_regular_limited(&lrc_path, 1024 * 1024)
        && let Ok(content) = String::from_utf8(bytes)
        && !content.trim().is_empty()
    {
        return Some(content);
    }

    let probe = ffprobe_json(path).ok()?;
    tag_value(
        &probe,
        &["unsyncedlyrics", "unsynclyrics", "lyrics", "syncedlyrics"],
    )
    .filter(|s| !s.is_empty())
}

fn ffprobe_json(path: &str) -> Result<serde_json::Value> {
    ffprobe_json_cancellable(path, &AtomicBool::new(false))
}
fn ffprobe_json_cancellable(path: &str, cancelled: &AtomicBool) -> Result<serde_json::Value> {
    let result = BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
            "--",
            path,
        ])
        .timeout(Duration::from_secs(30))
        .build()
        .run_cancellable(cancelled)
        .map_err(|e| match e {
            BigSubprocessError::Cancelled => Error::Cancelled,
            _ => Error::invalid_argument("audio_file", "Could not complete the media probe"),
        })?;
    if !result.status.success() {
        return Err(Error::invalid_argument("audio_file", "Media probe failed"));
    }
    serde_json::from_slice(&result.stdout)
        .map_err(|_| Error::invalid_argument("audio_file", "Invalid media probe response"))
}

fn first_audio_stream(root: &serde_json::Value) -> Option<&serde_json::Value> {
    root["streams"].as_array()?.iter().find(|stream| {
        stream["codec_type"]
            .as_str()
            .is_some_and(|kind| kind == "audio")
    })
}

fn probe_number(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn probe_i32(value: &serde_json::Value) -> Option<i32> {
    value
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn tag_value(root: &serde_json::Value, keys: &[&str]) -> Option<String> {
    tags_value(&root["format"]["tags"], keys).or_else(|| {
        root["streams"]
            .as_array()?
            .iter()
            .find_map(|stream| tags_value(&stream["tags"], keys))
    })
}

fn tags_value(tags: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let tag_fields = tags.as_object()?;
    for wanted in keys {
        for (key, value) in tag_fields {
            if key.eq_ignore_ascii_case(wanted)
                && let Some(text) = value.as_str()
                && !text.is_empty()
            {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn parse_tag_i32(value: &str) -> Option<i32> {
    value.split('/').next().map(str::trim)?.parse::<i32>().ok()
}

fn parse_year(value: &str) -> Option<i32> {
    value.get(0..4).unwrap_or(value).parse::<i32>().ok()
}

fn metadata_args(edits: &TagEdits) -> Vec<String> {
    let fields = [
        ("title", edits.title.clone()),
        ("artist", edits.artist.clone()),
        ("album", edits.album.clone()),
        ("album_artist", edits.album_artist.clone()),
        ("track", edits.track_number.map(|n| n.to_string())),
        ("disc", edits.disc_number.map(|n| n.to_string())),
        ("date", edits.year.clone()),
        ("genre", edits.genre.clone()),
    ];
    let mut args = Vec::new();
    for (key, value) in fields {
        if let Some(value) = value {
            args.push("-metadata".to_owned());
            args.push(format!("{key}={value}"));
        }
    }
    args
}

fn extract_embedded_album_art(path: &str, cancelled: &AtomicBool) -> Option<Vec<u8>> {
    let output = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            path,
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "pipe:1",
        ])
        .timeout(Duration::from_secs(30))
        .build()
        .run_cancellable(cancelled)
        .ok()?;
    if output.status.success() && !output.stdout.is_empty() {
        Some(output.stdout)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn tag_rewrite_replaces_title() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.flac");
        let made = Command::new("ffmpeg")
            .args([
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=0.2",
                "-y",
            ])
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        let path = path.to_str().unwrap();
        let edits = TagEdits {
            title: Some("Renamed".into()),
            ..TagEdits::default()
        };
        write_track_tags(path, &edits).unwrap();
        assert_eq!(
            read_track_metadata(path).unwrap().title.as_deref(),
            Some("Renamed")
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn fifo_sidecar_never_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("song.lrc");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let song = dir.path().join("song.mp3");
        assert!(extract_lyrics(song.to_str().unwrap()).is_none());
    }
}
