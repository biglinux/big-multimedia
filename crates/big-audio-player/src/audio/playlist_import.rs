//! Bounded native-path playlist import. Failure is distinct from an empty list,
//! and a list cut at [`MAX_PLAYLIST_ENTRIES`] says so.
use crate::media::iptv::{MAX_PLAYLIST_ENTRIES, PlaylistEntries};
use std::{io, path::Path};
const MAX_PLAYLIST_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn parse_playlist_file_for_import(path: &Path) -> io::Result<PlaylistEntries<String>> {
    let bytes = big_os_kit::storage::read_regular_limited(path, MAX_PLAYLIST_BYTES)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "playlist unavailable"))?;
    let content = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "playlist is not valid UTF-8"))?;
    parse_content(path, content)
}
/// What the person is told when a playlist was cut at [`MAX_PLAYLIST_ENTRIES`].
pub(crate) fn truncated_message() -> String {
    crate::i18n::i18n("Only the first {} entries of the playlist were loaded.")
        .replace("{}", &MAX_PLAYLIST_ENTRIES.to_string())
}
fn entry(base: &Path, line: &str) -> io::Result<String> {
    if line.is_empty() || line.len() > 4096 || line.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid or oversized playlist entry",
        ));
    }
    let uri = line.split_once(':').is_some_and(|(scheme, _)| {
        let mut chars = scheme.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if Path::new(line).is_absolute() || uri {
        return Ok(line.into());
    }
    base.join(line)
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "playlist path is not UTF-8"))
}
fn parse_content(path: &Path, content: &str) -> io::Result<PlaylistEntries<String>> {
    let base = path.parent().unwrap_or(Path::new("."));
    let mut result = PlaylistEntries::default();
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pls"))
    {
        let mut ordered = std::collections::BTreeMap::new();
        for line in content.lines() {
            if let Some((key, value)) = line.split_once('=')
                && let Some(index) = key.strip_prefix("File")
            {
                let index = index
                    .parse::<u32>()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid PLS index"))?;
                if index == 0 || ordered.insert(index, entry(base, value)?).is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "duplicate PLS index",
                    ));
                }
                // Keeps the lowest indices, so memory stays at the cap.
                if ordered.len() > MAX_PLAYLIST_ENTRIES {
                    ordered.pop_last();
                    result.truncated = true;
                }
            }
        }
        result.entries.extend(ordered.into_values());
    } else {
        for line in content.strip_prefix('\u{feff}').unwrap_or(content).lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            if !result.push(entry(base, line)?) {
                break;
            }
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_paths_preserve_spaces() {
        assert_eq!(
            parse_content(Path::new("/music/list.m3u"), "#EXTM3U\n song.ogg \n")
                .unwrap()
                .entries,
            ["/music/ song.ogg "]
        );
    }
    #[test]
    fn remote_uri_stays_remote() {
        assert_eq!(
            parse_content(Path::new("/music/l.m3u"), "https://host/song.mp3?x=1\n")
                .unwrap()
                .entries,
            ["https://host/song.mp3?x=1"]
        );
    }
    #[test]
    fn malformed_and_nul_entries_are_errors() {
        assert!(parse_content(Path::new("a.m3u"), "bad\0path").is_err());
        assert!(parse_content(Path::new("a.pls"), "File1=a\nFile1=b").is_err());
    }
    #[test]
    fn pls_is_sorted_by_index() {
        assert_eq!(
            parse_content(
                Path::new("/music/l.pls"),
                "[playlist]\nFile2=b.mp3\nFile1=a.mp3"
            )
            .unwrap()
            .entries,
            ["/music/a.mp3", "/music/b.mp3"]
        );
    }
    #[test]
    fn empty_is_distinct_from_unreadable() {
        assert!(
            parse_content(Path::new("/music/l"), "# empty")
                .unwrap()
                .entries
                .is_empty()
        );
    }
    // Malformed data and unreadable files are errors, not empty lists.
    #[cfg(not(miri))]
    struct TestDirectory(std::path::PathBuf);
    #[cfg(not(miri))]
    impl TestDirectory {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("playlist-import-r2-{}-{nonce}", std::process::id()));
            // Refuse collision; never remove or reuse a preexisting directory.
            std::fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }
    #[cfg(not(miri))]
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    #[cfg_attr(miri, ignore)]
    #[cfg(not(miri))]
    fn oversize_file_fails_before_reading() {
        let dir = TestDirectory::new();
        let path = dir.0.join("large.m3u");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_PLAYLIST_BYTES as u64 + 1)
            .unwrap();
        assert!(parse_playlist_file_for_import(&path).is_err());
    }
    #[test]
    #[cfg_attr(miri, ignore)]
    #[cfg(not(miri))]
    fn byte_limit_needs_valid_content() {
        let dir = TestDirectory::new();
        let path = dir.0.join("boundary.m3u");
        // A comment-only playlist is valid and empty even at the byte ceiling.
        std::fs::write(&path, vec![b'#'; MAX_PLAYLIST_BYTES]).unwrap();
        assert!(
            parse_playlist_file_for_import(&path)
                .unwrap()
                .entries
                .is_empty()
        );
        // A sparse NUL file of the same size is not a playlist.
        std::fs::write(&path, vec![0_u8; MAX_PLAYLIST_BYTES]).unwrap();
        assert!(parse_playlist_file_for_import(&path).is_err());
    }
    #[test]
    #[cfg_attr(miri, ignore)]
    #[cfg(not(miri))]
    fn missing_or_invalid_utf8_is_error() {
        let dir = TestDirectory::new();
        let path = dir.0.join("missing.m3u");
        assert_eq!(
            parse_playlist_file_for_import(&path).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        std::fs::write(&path, [0xff_u8, 0xfe]).unwrap();
        assert_eq!(
            parse_playlist_file_for_import(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    #[test]
    fn m3u_cap_truncates_and_reports() {
        let at_cap = "song.ogg\n".repeat(MAX_PLAYLIST_ENTRIES);
        let parsed = parse_content(Path::new("/music/list.m3u"), &at_cap).unwrap();
        assert_eq!((parsed.entries.len(), parsed.truncated), (100_000, false));
        let over = format!("{at_cap}last.ogg\n");
        let parsed = parse_content(Path::new("/music/list.m3u"), &over).unwrap();
        assert_eq!((parsed.entries.len(), parsed.truncated), (100_000, true));
        assert_eq!(parsed.entries.last().unwrap(), "/music/song.ogg");
    }
    #[test]
    fn pls_cap_keeps_lowest_indices() {
        let mut content = String::from("[playlist]\n");
        for index in (1..=MAX_PLAYLIST_ENTRIES + 1).rev() {
            content.push_str(&format!("File{index}={index}.mp3\n"));
        }
        let parsed = parse_content(Path::new("/music/l.pls"), &content).unwrap();
        assert_eq!((parsed.entries.len(), parsed.truncated), (100_000, true));
        assert_eq!(parsed.entries[0], "/music/1.mp3");
        assert_eq!(parsed.entries.last().unwrap(), "/music/100000.mp3");
    }
}
