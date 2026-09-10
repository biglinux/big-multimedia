// SPDX-License-Identifier: MIT

//! M3U / M3U8 parser.
//!
//! Recognised directives:
//!
//! - `#EXTM3U` — header (optional).
//! - `#EXTINF:<duration>,<title>` — metadata applied to the next entry.
//!
//! Lines starting with `#` other than the above are ignored. Empty
//! lines are skipped. Relative paths are resolved against `base_dir`
//! and rejected when they escape it (path-traversal protection).

use crate::limits::ParseLimits;
use crate::playlist::types::{ParseError, Playlist, PlaylistEntry, PlaylistTarget};
use std::path::{Component, Path};

/// Parse an M3U / M3U8 byte buffer relative to `base_dir`.
///
/// Pending `#EXTINF` metadata is attached to the next entry encountered
/// and then cleared. Recognised URL schemes (`http`, `https`, `rtsp`,
/// `rtmp`, `mms`, `file`) are emitted as [`PlaylistTarget::Url`]; every
/// other line is treated as a path and validated to lie within
/// `base_dir`.
///
/// # Errors
///
/// Returns [`ParseError::InputTooLarge`] when the byte length exceeds
/// [`ParseLimits::max_bytes`], [`ParseError::TooManyEntries`] when the
/// entry count exceeds [`ParseLimits::max_records`], and
/// [`ParseError::PathTraversal`] when a relative entry would escape
/// `base_dir`.
///
/// # Examples
///
/// Typical parse-then-iterate flow:
///
/// ```
/// use std::path::Path;
/// use big_media_engines::ParseLimits;
/// use big_media_engines::playlist::{parse_m3u, PlaylistTarget};
///
/// let src = b"\
/// #EXTM3U\n\
/// #EXTINF:213,Artist - Song\n\
/// song.mp3\n\
/// http://example.com/stream.mp3\n";
///
/// let pl = parse_m3u(src, Path::new("/home/u/Music"), ParseLimits::PLAYLIST)?;
/// assert_eq!(pl.entries.len(), 2);
/// assert_eq!(pl.entries[0].duration_secs, Some(213.0));
/// assert!(matches!(pl.entries[1].target, PlaylistTarget::Url(_)));
/// # Ok::<(), big_media_engines::playlist::ParseError>(())
/// ```
pub fn parse_m3u(
    bytes: &[u8],
    base_dir: &Path,
    limits: ParseLimits,
) -> Result<Playlist, ParseError> {
    if bytes.len() > limits.max_bytes {
        return Err(ParseError::InputTooLarge {
            len: bytes.len(),
            limit: limits.max_bytes,
        });
    }
    let text = String::from_utf8_lossy(strip_bom(bytes));
    let mut entries = Vec::new();
    let mut pending_duration: Option<f64> = None;
    let mut pending_title: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            // `<duration>,<title>` ; both halves optional.
            let mut split = rest.splitn(2, ',');
            if let Some(d) = split.next() {
                pending_duration = d.trim().parse().ok();
            }
            if let Some(t) = split.next() {
                let title = t.trim();
                if !title.is_empty() {
                    pending_title = Some(title.to_string());
                }
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let target = classify_target(line, base_dir)?;
        entries.push(PlaylistEntry {
            target,
            duration_secs: pending_duration.take(),
            title: pending_title.take(),
        });
        if entries.len() > limits.max_records {
            return Err(ParseError::TooManyEntries {
                count: entries.len(),
                limit: limits.max_records,
            });
        }
    }
    Ok(Playlist { entries })
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

fn classify_target(line: &str, base_dir: &Path) -> Result<PlaylistTarget, ParseError> {
    if is_url(line) {
        return Ok(PlaylistTarget::Url(line.to_string()));
    }
    let path = Path::new(line);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    if !resolved.is_absolute() && !is_safely_within(base_dir, &resolved) {
        return Err(ParseError::PathTraversal {
            offender: line.to_string(),
        });
    }
    // Reject `..` segments that would escape `base_dir`.
    let mut depth: i32 = 0;
    for comp in path.components() {
        match comp {
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            _ => {}
        }
        if depth < 0 {
            return Err(ParseError::PathTraversal {
                offender: line.to_string(),
            });
        }
    }
    Ok(PlaylistTarget::Path(resolved))
}

fn is_url(s: &str) -> bool {
    const SCHEMES: &[&str] = &[
        "http://", "https://", "rtsp://", "rtmp://", "mms://", "file://",
    ];
    SCHEMES.iter().any(|p| s.starts_with(p))
}

fn is_safely_within(base: &Path, candidate: &Path) -> bool {
    candidate.starts_with(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn limits() -> ParseLimits {
        ParseLimits::PLAYLIST
    }

    #[test]
    fn extm3u_header_and_relative_path() {
        let src = "#EXTM3U\n#EXTINF:123,Track Title\nfile.mp3\n";
        let pl = parse_m3u(src.as_bytes(), Path::new("/home/u/Music"), limits()).unwrap();
        assert_eq!(pl.entries.len(), 1);
        assert_eq!(pl.entries[0].duration_secs, Some(123.0));
        assert_eq!(pl.entries[0].title.as_deref(), Some("Track Title"));
        match &pl.entries[0].target {
            PlaylistTarget::Path(p) => assert_eq!(p, &PathBuf::from("/home/u/Music/file.mp3")),
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn http_url_classified_as_url() {
        let src = "http://example.com/stream.mp3\n";
        let pl = parse_m3u(src.as_bytes(), Path::new("/"), limits()).unwrap();
        match &pl.entries[0].target {
            PlaylistTarget::Url(u) => assert_eq!(u, "http://example.com/stream.mp3"),
            _ => panic!("expected Url"),
        }
    }

    #[test]
    fn path_traversal_with_parent_dir_rejected() {
        let src = "../../etc/passwd\n";
        let err = parse_m3u(src.as_bytes(), Path::new("/home/u/Music"), limits()).unwrap_err();
        assert!(matches!(err, ParseError::PathTraversal { .. }));
    }

    #[test]
    fn pending_metadata_consumed_per_entry() {
        let src = "#EXTINF:60,A\nfile1.mp3\nfile2.mp3\n";
        let pl = parse_m3u(src.as_bytes(), Path::new("/x"), limits()).unwrap();
        assert_eq!(pl.entries[0].duration_secs, Some(60.0));
        assert!(pl.entries[1].duration_secs.is_none());
        assert!(pl.entries[1].title.is_none());
    }

    #[test]
    fn input_too_large_rejected() {
        let small = ParseLimits {
            max_bytes: 5,
            max_records: 100,
        };
        let err = parse_m3u(b"#EXTM3U\nfile.mp3\n", Path::new("/x"), small).unwrap_err();
        assert!(matches!(err, ParseError::InputTooLarge { .. }));
    }

    #[test]
    fn too_many_entries_rejected() {
        let small = ParseLimits {
            max_bytes: 4096,
            max_records: 1,
        };
        let err = parse_m3u(b"a.mp3\nb.mp3\n", Path::new("/x"), small).unwrap_err();
        assert!(matches!(err, ParseError::TooManyEntries { .. }));
    }

    #[test]
    fn bom_is_stripped() {
        let mut buf = vec![0xEF, 0xBB, 0xBF];
        buf.extend_from_slice(b"file.mp3\n");
        let pl = parse_m3u(&buf, Path::new("/x"), limits()).unwrap();
        assert_eq!(pl.entries.len(), 1);
    }

    #[test]
    fn comments_are_skipped() {
        let src = "#playlist generated by big-app\n#some comment\nfile.mp3\n";
        let pl = parse_m3u(src.as_bytes(), Path::new("/x"), limits()).unwrap();
        assert_eq!(pl.entries.len(), 1);
    }

    #[test]
    fn internal_parent_dir_staying_within_base_is_accepted() {
        // `a/../b.mp3` dips to depth 0 at the `..` but never goes negative,
        // so it stays inside `base_dir` and must be accepted. Pins the depth
        // accounting (Normal `+= 1`, the `depth < 0` boundary) against
        // off-by-one mutations that would wrongly reject a safe path.
        let src = "a/../b.mp3\n";
        let pl = parse_m3u(src.as_bytes(), Path::new("/home/u/Music"), limits()).unwrap();
        assert_eq!(pl.entries.len(), 1);
        match &pl.entries[0].target {
            PlaylistTarget::Path(p) => {
                assert_eq!(p, &PathBuf::from("/home/u/Music/a/../b.mp3"));
            }
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn imbalanced_parent_dir_escaping_base_is_rejected() {
        // `a/../../x.mp3`: depth 1 -> 0 -> -1 escapes the base directory.
        // Complements `path_traversal_with_parent_dir_rejected` (which trips
        // on the very first `..`) by forcing a Normal increment first.
        let src = "a/../../x.mp3\n";
        let err = parse_m3u(src.as_bytes(), Path::new("/home/u/Music"), limits()).unwrap_err();
        assert!(matches!(err, ParseError::PathTraversal { .. }));
    }

    #[test]
    fn relative_base_dir_accepts_simple_entry() {
        // A relative `base_dir` is the only way `is_safely_within` becomes the
        // deciding check (with an absolute base the joined path is absolute and
        // that branch is skipped). Asserts the in-bounds path is accepted.
        let src = "file.mp3\n";
        let pl = parse_m3u(src.as_bytes(), Path::new("Music"), limits()).unwrap();
        assert_eq!(pl.entries.len(), 1);
        match &pl.entries[0].target {
            PlaylistTarget::Path(p) => assert_eq!(p, &PathBuf::from("Music/file.mp3")),
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn input_exactly_at_byte_limit_is_accepted() {
        // Boundary: len == max_bytes must pass (the guard is strictly `>`).
        let src = b"a.mp3\n";
        let exact = ParseLimits {
            max_bytes: src.len(),
            max_records: 100,
        };
        let pl = parse_m3u(src, Path::new("/x"), exact).unwrap();
        assert_eq!(pl.entries.len(), 1);
    }

    #[test]
    fn entry_count_exactly_at_limit_is_accepted() {
        // Boundary: exactly max_records entries must pass (guard is strictly `>`).
        let exact = ParseLimits {
            max_bytes: 4096,
            max_records: 2,
        };
        let pl = parse_m3u(b"a.mp3\nb.mp3\n", Path::new("/x"), exact).unwrap();
        assert_eq!(pl.entries.len(), 2);
    }

    #[test]
    fn parse_error_display_messages() {
        // Pins the Display impl against a `Ok(Default::default())` (empty) mutant.
        assert_eq!(
            ParseError::PathTraversal {
                offender: "../x".into(),
            }
            .to_string(),
            "playlist path escapes base: ../x"
        );
        assert_eq!(
            ParseError::TooManyEntries { count: 9, limit: 5 }.to_string(),
            "playlist too many entries: 9, limit 5"
        );
    }
}
