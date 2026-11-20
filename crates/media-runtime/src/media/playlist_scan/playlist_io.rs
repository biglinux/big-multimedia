use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use big_os_kit::url::{ListOrigin, accepts_list_entry, file_uri_path, is_stream_uri};

use crate::error::Result;
use crate::media::iptv::{MAX_PLAYLIST_ENTRIES, PlaylistEntries, decode_playlist};
use crate::media::playlist::display_name;

/// Largest playlist file read.
const MAX_PLAYLIST_FILE_BYTES: usize = 16 * 1024 * 1024;

/// The entries of the M3U or PLS file at `path` (PLS by its extension), in
/// order, relative ones resolved against its folder. An entry that is not a
/// stream URI or an existing file outside the system directories is left out
/// and counted in [`PlaylistEntries::skipped`]. Blocking.
///
/// # Errors
/// The file is missing, unreadable, larger than 16 MiB or holds a NUL byte;
/// a PLS file has an invalid or repeated `File` index.
pub fn parse_playlist_file(path: &Path) -> io::Result<PlaylistEntries<String>> {
    let bytes = big_os_kit::storage::read_regular_limited(
        &std::fs::canonicalize(path)?,
        MAX_PLAYLIST_FILE_BYTES,
    )?
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "playlist unavailable"))?;
    if bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "playlist is not text",
        ));
    }
    let content = decode_playlist(bytes);
    let is_pls = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pls"));
    let (lines, truncated) = if is_pls {
        pls_lines(&content)?
    } else {
        m3u_lines(&content)
    };
    let base = path.parent().unwrap_or(Path::new("/"));
    let accepted = |line: &str| {
        resolve_playlist_entry(base, line)
            .filter(|entry| accepts_list_entry(entry, ListOrigin::LocalFile))
    };
    let mut result = PlaylistEntries {
        truncated,
        ..PlaylistEntries::default()
    };
    for line in lines {
        // Names may start or end with blanks; a sloppy list indents its lines.
        let trimmed = line.trim();
        let mut entry = accepted(line);
        if entry.is_none() && trimmed != line {
            entry = accepted(trimmed);
        }
        match entry {
            Some(entry) => result.entries.push(entry),
            None => result.skipped += 1,
        }
    }
    Ok(result)
}

/// Every non-comment line, at most [`MAX_PLAYLIST_ENTRIES`], and whether more
/// were left.
fn m3u_lines(content: &str) -> (Vec<&str>, bool) {
    let mut lines = content
        .strip_prefix('\u{feff}')
        .unwrap_or(content)
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
    let kept: Vec<&str> = lines.by_ref().take(MAX_PLAYLIST_ENTRIES).collect();
    let truncated = lines.next().is_some();
    (kept, truncated)
}

/// The `FileN=` values in index order, the lowest [`MAX_PLAYLIST_ENTRIES`] of
/// them, and whether more were left.
fn pls_lines(content: &str) -> io::Result<(Vec<&str>, bool)> {
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidData, message);
    let mut ordered = BTreeMap::new();
    let mut truncated = false;
    for line in content.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let Some(index) = key
            .get(..4)
            .filter(|prefix| prefix.eq_ignore_ascii_case("file"))
            .and_then(|_| key.get(4..))
        else {
            continue;
        };
        let index = index
            .parse::<u32>()
            .ok()
            .filter(|index| *index > 0)
            .ok_or_else(|| invalid("invalid PLS index"))?;
        if ordered.insert(index, value).is_some() {
            return Err(invalid("duplicate PLS index"));
        }
        if ordered.len() > MAX_PLAYLIST_ENTRIES {
            ordered.pop_last();
            truncated = true;
        }
    }
    Ok((ordered.into_values().collect(), truncated))
}

/// `entry` as mpv would open it: a stream URI or an absolute path as it is, a
/// `file://` URI as its path, anything else joined to `base`.
fn resolve_playlist_entry(base: &Path, entry: &str) -> Option<String> {
    if is_stream_uri(entry) || entry.starts_with('/') {
        return Some(entry.to_owned());
    }
    file_uri_path(entry)
        .unwrap_or_else(|| base.join(entry))
        .into_os_string()
        .into_string()
        .ok()
}

/// Extensions of the files [`write_m3u`] writes, offered by export pickers.
pub const M3U_EXTENSIONS: &[&str] = &["m3u8", "m3u"];

/// Worker-only export. Never overwrite an existing user document and never
/// report success from an ignored filesystem error. Embedded newlines in paths
/// cannot be represented faithfully in M3U and are rejected.
pub fn write_m3u(path: &Path, items: &[String]) -> Result<()> {
    const MAX_BYTES: usize = 16 * 1024 * 1024;
    if items.len() > MAX_PLAYLIST_ENTRIES {
        return Err(crate::Error::internal("Too many playlist entries"));
    }
    let mut content = String::from("#EXTM3U\n");
    for item in items {
        if item.bytes().any(|b| matches!(b, 0 | b'\r' | b'\n')) {
            return Err(crate::Error::internal(
                "A playlist path contains a line break",
            ));
        }
        let name = display_name(item).replace(['\r', '\n'], " ");
        if content.len() + item.len() + name.len() + 16 > MAX_BYTES {
            return Err(crate::Error::internal("Playlist export exceeds its limit"));
        }
        content.push_str(&format!("#EXTINF:0,{name}\n{item}\n"));
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let stage = big_os_kit::storage::StagedFile::new(path)?;
    std::fs::write(stage.path(), content)?;
    stage.publish(path)?.into_io_result()?;
    Ok(())
}

#[must_use]
pub fn is_playlist_file(path: &str) -> bool {
    big_media_components::media::files::is_playlist_file(path)
}
