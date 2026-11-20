use super::*;
use crate::media::iptv::PlaylistEntries;
use std::io;
use std::path::Path;

/// `content` written to `name` in a new directory, which the caller keeps.
fn playlist(name: &str, content: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, content).unwrap();
    (dir, path)
}

fn entries(name: &str, content: &str) -> PlaylistEntries<String> {
    let (_dir, path) = playlist(name, content.as_bytes());
    parse_playlist_file(&path).unwrap()
}

#[test]
fn m3u_keeps_stream_urls() {
    let list = entries(
        "test.m3u",
        "#EXTM3U\n#EXTINF:-1,Station\nhttp://stream.example.com/live\n\n# comment\n  #EXTINF:0,x\nHTTPS://secure.example.com/audio.mp3\n",
    );
    assert_eq!(
        (list.entries, list.skipped),
        (
            vec![
                "http://stream.example.com/live".to_string(),
                "HTTPS://secure.example.com/audio.mp3".to_string()
            ],
            0
        )
    );
}

#[test]
fn local_entries_resolve_against_the_list() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["song.mp3", " spaced.ogg ", "a b.flac"] {
        std::fs::write(dir.path().join(name), b"").unwrap();
    }
    let folder = dir.path().to_str().unwrap();
    let m3u = dir.path().join("list.m3u");
    std::fs::write(
        &m3u,
        format!(
            "song.mp3\n spaced.ogg \n  {folder}/song.mp3\nfile://{}/a%20b.flac\nmissing.mp3\n",
            folder.replace(' ', "%20")
        ),
    )
    .unwrap();
    let list = parse_playlist_file(&m3u).unwrap();
    assert_eq!(
        (list.entries, list.skipped),
        (
            vec![
                format!("{folder}/song.mp3"),
                format!("{folder}/ spaced.ogg "),
                format!("{folder}/song.mp3"),
                format!("{folder}/a b.flac"),
            ],
            1
        )
    );
}

#[test]
fn pls_is_read_in_index_order() {
    let list = entries(
        "radio.pls",
        "[playlist]\nFile2=http://stream2.example.com\nTitle2=Two\nfile1=http://stream1.example.com\nNumberOfEntries=2\n",
    );
    assert_eq!(
        list.entries,
        ["http://stream1.example.com", "http://stream2.example.com"]
    );
}

#[test]
fn malformed_files_are_errors() {
    for (name, content) in [
        ("nul.m3u", &b"http://a/1\nbad\0path\n"[..]),
        ("twice.pls", b"File1=http://a\nFile1=http://b\n"),
        ("zero.pls", b"File0=http://a\n"),
        ("word.pls", b"FileX=http://a\n"),
    ] {
        let (_dir, path) = playlist(name, content);
        assert_eq!(
            parse_playlist_file(&path).map_err(|error| error.kind()),
            Err(io::ErrorKind::InvalidData),
            "{name}"
        );
    }
    assert_eq!(
        parse_playlist_file(Path::new("/nonexistent/playlist.m3u")).map_err(|e| e.kind()),
        Err(io::ErrorKind::NotFound)
    );
}

#[test]
fn comment_only_list_is_empty() {
    let list = entries("empty.m3u", "#EXTM3U\n# nothing\n");
    assert!(list.entries.is_empty() && list.skipped == 0 && !list.truncated);
}

#[test]
fn latin1_list_still_reads() {
    let (_dir, path) = playlist("old.m3u", b"#EXTINF:-1,Not\xedcias\nhttp://a/not\xedcias\n");
    assert_eq!(
        parse_playlist_file(&path).unwrap().entries,
        ["http://a/notícias"]
    );
}

#[test]
fn oversize_file_is_refused() {
    let (_dir, path) = playlist("large.m3u", b"");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    assert!(parse_playlist_file(&path).is_err());
    std::fs::write(&path, vec![b'#'; 16 * 1024 * 1024]).unwrap();
    assert!(parse_playlist_file(&path).unwrap().entries.is_empty());
}

#[test]
fn entry_caps_keep_the_first_entries() {
    let at_cap = "http://a/\n".repeat(100_000);
    let list = entries("list.m3u", &at_cap);
    assert_eq!((list.entries.len(), list.truncated), (100_000, false));
    let list = entries("list.m3u", &format!("{at_cap}http://b/\n"));
    assert_eq!((list.entries.len(), list.truncated), (100_000, true));
    assert_eq!(list.entries.last().unwrap(), "http://a/");

    let mut pls = String::from("[playlist]\n");
    for index in (1..=100_001).rev() {
        pls.push_str(&format!("File{index}=http://a/{index}\n"));
    }
    let list = entries("long.pls", &pls);
    assert_eq!((list.entries.len(), list.truncated), (100_000, true));
    assert_eq!(list.entries[0], "http://a/1");
    assert_eq!(list.entries.last().unwrap(), "http://a/100000");
}

proptest::proptest! {
    // Any file name the writer accepts, blanks, `#` and `%` included, reads back.
    #[test]
    fn written_names_read_back(
        names in proptest::collection::btree_set("[ #%a-z0-9\\t.:\u{a0}é\u{feff}-]{1,12}", 1..6)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        for name in names.iter().filter(|name| !matches!(name.as_str(), "." | "..")) {
            let path = dir.path().join(name);
            std::fs::write(&path, b"fake").unwrap();
            files.push(path.to_str().unwrap().to_owned());
        }
        let m3u = dir.path().join("list.m3u8");
        write_m3u(&m3u, &files).unwrap();
        proptest::prop_assert_eq!(parse_playlist_file(&m3u).unwrap().entries, files);
    }
}

#[test]
fn write_m3u_refuses_line_breaks() {
    let dir = tempfile::tempdir().unwrap();
    let m3u = dir.path().join("list.m3u8");
    assert!(write_m3u(&m3u, &["/a\n/etc/passwd".to_string()]).is_err());
    assert!(!m3u.exists());
}

#[test]
fn hostile_entries_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let control = dir.path().join("control.mp3");
    std::fs::write(&control, b"fake").unwrap();
    let control = control.to_string_lossy().into_owned();
    let link = dir.path().join("hostname.mp3");
    std::os::unix::fs::symlink("/etc/hostname", &link).unwrap();
    let to_root = "../".repeat(dir.path().components().count());
    let written = dir.path().join("written");
    let rows = [
        "/etc/passwd".to_string(),
        format!("{to_root}etc/passwd"),
        link.to_string_lossy().into_owned(),
        "/proc/self/environ".to_string(),
        "/dev/stdin".to_string(),
        "/etc".to_string(),
        "file:///etc/passwd".to_string(),
        format!(
            "av://lavfi:sine,ametadata=mode=print:file={}",
            written.display()
        ),
        "fdclose://0".to_string(),
        "memory://#EXTM3U".to_string(),
    ];
    let m3u = dir.path().join("hostile.m3u");
    let pls = dir.path().join("hostile.pls");
    let mut leaks = Vec::new();
    for row in &rows {
        std::fs::write(&m3u, format!("#EXTM3U\n{control}\n{row}\n")).unwrap();
        std::fs::write(&pls, format!("[playlist]\nFile1={control}\nFile2={row}\n")).unwrap();
        for path in [&m3u, &pls] {
            let list = parse_playlist_file(path).unwrap();
            if list.entries != [control.clone()] || list.skipped != 1 {
                leaks.push(format!("{} {row}: {list:?}", path.display()));
            }
        }
    }
    assert!(leaks.is_empty(), "entries kept: {leaks:#?}");
}
