use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessSpec};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
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
    let art_hash = extract_album_art_cancellable(path, cancelled).map(|data| {
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

pub fn write_track_tags(path: &str, edits: &TagEdits) -> Result<()> {
    let source = Path::new(path);
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let suffix = source
        .extension()
        .and_then(|ext| ext.to_str())
        .map_or_else(String::new, |ext| format!(".{ext}"));
    let temp = tempfile::Builder::new()
        .prefix(".big-media-tags-")
        .suffix(&suffix)
        .tempfile_in(parent)
        .map_err(|e| Error::internal(format!("cannot create tag temp file: {e}")))?;
    let temp_path = temp.path().to_path_buf();
    drop(temp);

    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]);
    command.arg(source);
    command.args(["-map", "0", "-c", "copy", "-map_metadata", "0"]);
    add_metadata_args(&mut command, edits);
    command.arg(&temp_path);
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::piped());

    let output = command
        .output()
        .map_err(|e| Error::internal(format!("failed to spawn ffmpeg: {e}")))?;
    if !output.status.success() {
        let _ = fs::remove_file(&temp_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::internal(format!(
            "ffmpeg tag rewrite failed: {stderr}"
        )));
    }
    if let Ok(meta) = fs::metadata(source) {
        let _ = fs::set_permissions(&temp_path, meta.permissions());
    }
    fs::rename(&temp_path, source)
        .map_err(|e| Error::internal(format!("failed to promote rewritten tags: {e}")))?;
    sync_parent_dir(parent);
    Ok(())
}

/// Embedded picture first; falls back to sibling Cover/folder/front/album/
/// albumart/thumb × jpg/jpeg/png/webp.
pub fn extract_album_art(path: &str) -> Option<Vec<u8>> {
    extract_album_art_cancellable(path, &AtomicBool::new(false))
}
fn extract_album_art_cancellable(path: &str, cancelled: &AtomicBool) -> Option<Vec<u8>> {
    if cancelled.load(Ordering::Acquire) {
        return None;
    }
    if let Some(data) = extract_embedded_album_art(path, cancelled) {
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
    let lrc_path = std::path::Path::new(path).with_extension("lrc");
    if lrc_path.exists()
        && let Ok(content) = std::fs::read_to_string(&lrc_path)
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

fn add_metadata_args(command: &mut Command, edits: &TagEdits) {
    if let Some(ref value) = edits.title {
        command.args(["-metadata", &format!("title={value}")]);
    }
    if let Some(ref value) = edits.artist {
        command.args(["-metadata", &format!("artist={value}")]);
    }
    if let Some(ref value) = edits.album {
        command.args(["-metadata", &format!("album={value}")]);
    }
    if let Some(ref value) = edits.album_artist {
        command.args(["-metadata", &format!("album_artist={value}")]);
    }
    if let Some(value) = edits.track_number {
        command.args(["-metadata", &format!("track={value}")]);
    }
    if let Some(value) = edits.disc_number {
        command.args(["-metadata", &format!("disc={value}")]);
    }
    if let Some(ref value) = edits.year {
        command.args(["-metadata", &format!("date={value}")]);
    }
    if let Some(ref value) = edits.genre {
        command.args(["-metadata", &format!("genre={value}")]);
    }
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

fn sync_parent_dir(parent: &Path) {
    if let Ok(mut dir) = fs::File::open(parent) {
        let _ = dir.flush();
        let _ = dir.sync_all();
    }
}
