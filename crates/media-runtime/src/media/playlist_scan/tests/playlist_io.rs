use super::*;
use std::path::Path;

#[test]
fn parse_m3u_extracts_urls() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("test.m3u");
    std::fs::write(
        &m3u,
        "#EXTM3U\n#EXTINF:-1,Station\nhttp://stream.example.com/live\nhttps://secure.example.com/audio.mp3\n",
    )
    .unwrap();
    let result = parse_m3u(&m3u).entries;
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], "http://stream.example.com/live");
    assert_eq!(result[1], "https://secure.example.com/audio.mp3");
}

#[test]
fn parse_m3u_resolves_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("song.mp3");
    std::fs::write(&audio, "fake-audio").unwrap();
    let m3u = dir.path().join("playlist.m3u");
    std::fs::write(&m3u, "#EXTM3U\nsong.mp3\n").unwrap();
    let result = parse_m3u(&m3u).entries;
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("song.mp3"));
    assert!(result[0].starts_with(dir.path().to_str().unwrap()));
}

#[test]
fn parse_m3u_skips_nonexistent_files() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("test.m3u");
    std::fs::write(&m3u, "nonexistent.mp3\n").unwrap();
    let result = parse_m3u(&m3u).entries;
    assert!(result.is_empty());
}

#[test]
fn parse_m3u_skips_comments_and_empty() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("test.m3u");
    std::fs::write(
        &m3u,
        "#EXTM3U\n\n# comment\n#EXTINF:0,Dummy\nhttps://example.com/stream\n\n",
    )
    .unwrap();
    let result = parse_m3u(&m3u).entries;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "https://example.com/stream");
}

#[test]
fn parse_m3u_nonexistent_file_returns_empty() {
    let result = parse_m3u(Path::new("/nonexistent/playlist.m3u")).entries;
    assert!(result.is_empty());
}

#[test]
fn parse_playlist_file_pls_format() {
    let dir = tempfile::tempdir().unwrap();
    let pls = dir.path().join("radio.pls");
    std::fs::write(
        &pls,
        "[playlist]\nFile1=http://stream.example.com/radio\nTitle1=Radio Station\n",
    )
    .unwrap();
    let result = parse_playlist_file(pls.to_str().unwrap()).entries;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "http://stream.example.com/radio");
}

#[test]
fn parse_playlist_file_m3u_format() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("list.m3u");
    std::fs::write(&m3u, "#EXTM3U\nhttps://example.com/audio.mp3\n").unwrap();
    let result = parse_playlist_file(m3u.to_str().unwrap()).entries;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "https://example.com/audio.mp3");
}

#[test]
fn parse_pls_multiple_entries() {
    let dir = tempfile::tempdir().unwrap();
    let pls = dir.path().join("multi.pls");
    std::fs::write(
        &pls,
        concat!(
            "[playlist]\n",
            "File1=http://stream1.example.com\n",
            "Title1=Station 1\n",
            "File2=http://stream2.example.com\n",
            "Title2=Station 2\n",
            "NumberOfEntries=2\n",
        ),
    )
    .unwrap();
    let result = crate::media::playlist_scan::playlist_io::parse_pls_file(&pls).entries;
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], "http://stream1.example.com");
    assert_eq!(result[1], "http://stream2.example.com");
}

#[test]
fn parse_pls_case_insensitive_key() {
    let dir = tempfile::tempdir().unwrap();
    let pls = dir.path().join("case.pls");
    std::fs::write(&pls, "FILE1=https://example.com/stream\n").unwrap();
    let result = crate::media::playlist_scan::playlist_io::parse_pls_file(&pls).entries;
    assert_eq!(result.len(), 1);
}

#[test]
fn parse_pls_nonexistent_returns_empty() {
    let result = crate::media::playlist_scan::playlist_io::parse_pls_file(Path::new(
        "/nonexistent/radio.pls",
    ));
    assert!(result.entries.is_empty());
}

#[test]
fn parse_pls_stops_at_entry_cap() {
    let dir = tempfile::tempdir().unwrap();
    let pls = dir.path().join("long.pls");
    let mut lines = String::new();
    for index in 0..=100_000 {
        lines.push_str(&format!("File{index}=http://a/{index}\n"));
    }
    std::fs::write(&pls, lines).unwrap();
    let result = crate::media::playlist_scan::playlist_io::parse_pls_file(&pls);
    assert_eq!((result.entries.len(), result.truncated), (100_000, true));
    assert_eq!(result.entries.last().unwrap(), "http://a/99999");
}

#[test]
fn write_m3u_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<String> = (0..3)
        .map(|i| {
            let path = dir.path().join(format!("track {i}.mp3"));
            std::fs::write(&path, b"fake").unwrap();
            path.to_string_lossy().into_owned()
        })
        .collect();
    let m3u = dir.path().join("list.m3u8");
    write_m3u(&m3u, &files).unwrap();
    assert_eq!(parse_m3u(&m3u).entries, files);
}

#[test]
fn write_m3u_refuses_line_breaks() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("list.m3u8");
    assert!(write_m3u(&m3u, &["/a\n/etc/passwd".to_string()]).is_err());
    assert!(!m3u.exists());
}
