//! Bounded native-path playlist import. Failure is distinct from an empty list.
use std::{io, path::Path};
const MAX_PLAYLIST_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

pub(crate) fn parse_playlist_file_for_import(path: &Path) -> io::Result<Vec<String>> {
    let bytes = big_os_kit::storage::read_regular_limited(path, MAX_PLAYLIST_BYTES)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "playlist unavailable"))?;
    let content = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "playlist is not valid UTF-8"))?;
    parse_content(path, content)
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
fn parse_content(path: &Path, content: &str) -> io::Result<Vec<String>> {
    let base = path.parent().unwrap_or(Path::new("."));
    let mut result = Vec::new();
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
                if ordered.len() > MAX_ENTRIES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "too many playlist entries",
                    ));
                }
            }
        }
        result.extend(ordered.into_values());
    } else {
        for line in content.strip_prefix('\u{feff}').unwrap_or(content).lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            result.push(entry(base, line)?);
            if result.len() > MAX_ENTRIES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "too many playlist entries",
                ));
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
            parse_content(Path::new("/music/list.m3u"), "#EXTM3U\n song.ogg \n").unwrap(),
            ["/music/ song.ogg "]
        );
    }
    #[test]
    fn remote_uri_is_not_made_into_a_local_path() {
        assert_eq!(
            parse_content(Path::new("/music/l.m3u"), "https://host/song.mp3?x=1\n").unwrap(),
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
            .unwrap(),
            ["/music/a.mp3", "/music/b.mp3"]
        );
    }
    #[test]
    fn empty_is_distinct_from_unreadable() {
        assert!(
            parse_content(Path::new("/music/l"), "# empty")
                .unwrap()
                .is_empty()
        );
    }
    // Preserve boundary coverage from the old parser while correcting its
    // contract: malformed data and unreadable files are errors, not empty lists.
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
    fn oversize_file_is_an_error_without_reading_its_entire_body() {
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
    fn exact_byte_limit_is_valid_only_with_valid_content() {
        let dir = TestDirectory::new();
        let path = dir.0.join("boundary.m3u");
        // A comment-only playlist is valid and empty even at the byte ceiling.
        std::fs::write(&path, vec![b'#'; MAX_PLAYLIST_BYTES]).unwrap();
        assert!(parse_playlist_file_for_import(&path).unwrap().is_empty());
        // The previous test accepted a sparse NUL file at this same size.
        std::fs::write(&path, vec![0_u8; MAX_PLAYLIST_BYTES]).unwrap();
        assert!(parse_playlist_file_for_import(&path).is_err());
    }
    #[test]
    #[cfg_attr(miri, ignore)]
    #[cfg(not(miri))]
    fn missing_and_invalid_utf8_are_errors_not_empty_playlists() {
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
    fn entry_count_is_bounded() {
        let content = "song.ogg\n".repeat(MAX_ENTRIES + 1);
        assert!(parse_content(Path::new("/music/list.m3u"), &content).is_err());
    }
}
