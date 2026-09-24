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
    let result = parse_m3u(&m3u);
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
    let result = parse_m3u(&m3u);
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("song.mp3"));
    assert!(result[0].starts_with(dir.path().to_str().unwrap()));
}

#[test]
fn parse_m3u_skips_nonexistent_files() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("test.m3u");
    std::fs::write(&m3u, "nonexistent.mp3\n").unwrap();
    let result = parse_m3u(&m3u);
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
    let result = parse_m3u(&m3u);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "https://example.com/stream");
}

#[test]
fn parse_m3u_nonexistent_file_returns_empty() {
    let result = parse_m3u(Path::new("/nonexistent/playlist.m3u"));
    assert!(result.is_empty());
}

#[test]
fn parse_iptv_m3u_extracts_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("iptv.m3u");
    std::fs::write(
        &m3u,
        concat!(
            "#EXTM3U\n",
            "#EXTINF:-1 tvg-logo=\"http://logo.com/ch1.png\" group-title=\"News\",Channel One\n",
            "http://stream.example.com/ch1\n",
            "#EXTINF:-1 group-title=\"Sports\",Channel Two\n",
            "http://stream.example.com/ch2\n",
        ),
    )
    .unwrap();
    let channels = crate::media::iptv::parse_m3u_file(&m3u);
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0].name, "Channel One");
    assert_eq!(channels[0].group, "News");
    assert_eq!(channels[0].logo, "http://logo.com/ch1.png");
    assert_eq!(channels[0].url, "http://stream.example.com/ch1");
    assert_eq!(channels[1].name, "Channel Two");
    assert_eq!(channels[1].group, "Sports");
    assert!(channels[1].logo.is_empty());
}

#[test]
fn parse_iptv_m3u_url_without_extinf() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("iptv.m3u");
    std::fs::write(&m3u, "#EXTM3U\nhttp://stream.example.com/direto\n").unwrap();
    let channels = crate::media::iptv::parse_m3u_file(&m3u);
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].url, "http://stream.example.com/direto");
    assert!(!channels[0].name.is_empty());
}

#[test]
fn parse_iptv_m3u_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("empty.m3u");
    std::fs::write(&m3u, "").unwrap();
    assert!(crate::media::iptv::parse_m3u_file(&m3u).is_empty());
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
    let result = parse_playlist_file(pls.to_str().unwrap());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "http://stream.example.com/radio");
}

#[test]
fn parse_playlist_file_m3u_format() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("list.m3u");
    std::fs::write(&m3u, "#EXTM3U\nhttps://example.com/audio.mp3\n").unwrap();
    let result = parse_playlist_file(m3u.to_str().unwrap());
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
    let result = crate::media::playlist_scan::playlist_io::parse_pls_file(&pls);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], "http://stream1.example.com");
    assert_eq!(result[1], "http://stream2.example.com");
}

#[test]
fn parse_pls_case_insensitive_key() {
    let dir = tempfile::tempdir().unwrap();
    let pls = dir.path().join("case.pls");
    std::fs::write(&pls, "FILE1=https://example.com/stream\n").unwrap();
    let result = crate::media::playlist_scan::playlist_io::parse_pls_file(&pls);
    assert_eq!(result.len(), 1);
}

#[test]
fn parse_pls_nonexistent_returns_empty() {
    let result = crate::media::playlist_scan::playlist_io::parse_pls_file(Path::new(
        "/nonexistent/radio.pls",
    ));
    assert!(result.is_empty());
}
