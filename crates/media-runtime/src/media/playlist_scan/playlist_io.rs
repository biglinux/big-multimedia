use std::path::Path;

use crate::error::Result;
use crate::media::iptv::{IptvChannel, parse_m3u_file};
use crate::media::playlist::display_name;

/// Filters non-existent local entries; URLs always pass.
pub fn parse_m3u(path: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let base_dir = path.parent().unwrap_or_else(|| Path::new("/"));
    content
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| resolve_playlist_entry(base_dir, line.trim()))
        .filter(|entry| is_resolved_playlist_entry_valid(entry))
        .collect()
}

pub fn parse_iptv_m3u(path: &Path) -> Vec<IptvChannel> {
    parse_m3u_file(path)
}

/// Worker-only export. Never overwrite an existing user document and never
/// report success from an ignored filesystem error. Embedded newlines in paths
/// cannot be represented faithfully in M3U and are rejected.
pub fn write_m3u(path: &Path, items: &[String]) -> Result<()> {
    const MAX_BYTES:usize=16*1024*1024;
    if items.len()>100_000 {return Err(crate::Error::internal("Too many playlist entries"));}
    let mut content=String::from("#EXTM3U\n");
    for item in items {
        if item.bytes().any(|b|matches!(b,0|b'\r'|b'\n')) {return Err(crate::Error::internal("A playlist path contains a line break"));}
        let name=display_name(item).replace(['\r','\n']," ");
        if content.len()+item.len()+name.len()+16>MAX_BYTES {return Err(crate::Error::internal("Playlist export exceeds its limit"));}
        content.push_str(&format!("#EXTINF:0,{name}\n{item}\n"));
    }
    if let Some(parent)=path.parent().filter(|p|!p.as_os_str().is_empty()) {std::fs::create_dir_all(parent)?;}
    let stage=big_os_kit::storage::StagedFile::new(path)?;
    std::fs::write(stage.path(),content)?;
    stage.publish(path)?.into_io_result()?;
    Ok(())
}

#[must_use]
pub fn is_playlist_file(path: &str) -> bool {
    big_media_components::media::files::is_playlist_file(path)
}

/// Dispatches by extension: `.pls` → INI parser, anything else → M3U.
pub fn parse_playlist_file(path: &str) -> Vec<String> {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext == "pls" {
        parse_pls_file(p)
    } else {
        parse_m3u(p)
    }
}

pub fn export_m3u(paths: &[String], output_path: &str) -> Result<()> {
    write_m3u(Path::new(output_path), paths)
}

pub(crate) fn parse_pls_file(path: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let base_dir = path.parent().unwrap_or_else(|| Path::new("/"));
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.to_ascii_lowercase().starts_with("file") {
                line.split_once('=')
                    .map(|(_, value)| resolve_playlist_entry(base_dir, value.trim()))
            } else {
                None
            }
        })
        .collect()
}

fn resolve_playlist_entry(base_dir: &Path, entry: &str) -> String {
    if entry.starts_with("http://")
        || entry.starts_with("https://")
        || entry.starts_with("rtsp://")
        || entry.starts_with("rtmp://")
        || entry.starts_with("mms://")
    {
        entry.to_string()
    } else {
        let entry_path = Path::new(entry);
        if entry_path.is_absolute() {
            entry.to_string()
        } else {
            base_dir.join(entry).to_string_lossy().to_string()
        }
    }
}

/// Paths under these prefixes are never legitimate media targets and a
/// crafted playlist that resolves into them is almost certainly an
/// exfiltration attempt (file-existence probe via mpv error reporting
/// or subtitle/cover loaders).
const SYSTEM_PATH_DENY: &[&str] = &["/etc/", "/proc/", "/sys/", "/dev/", "/root/", "/boot/"];

fn is_resolved_playlist_entry_valid(entry: &str) -> bool {
    if entry.starts_with("http://")
        || entry.starts_with("https://")
        || entry.starts_with("rtsp://")
        || entry.starts_with("rtmp://")
        || entry.starts_with("mms://")
    {
        return true;
    }
    // canonicalize collapses `..` and follows symlinks; on failure
    // (broken symlink, missing file) the entry is dropped — same
    // user-visible behavior as before.
    let Ok(canon) = std::fs::canonicalize(entry) else {
        return false;
    };
    let canon_str = canon.to_string_lossy();
    !SYSTEM_PATH_DENY
        .iter()
        .any(|deny| canon_str.starts_with(deny))
}
