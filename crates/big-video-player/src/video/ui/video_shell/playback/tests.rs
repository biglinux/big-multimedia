mod uri_and_seek_tests {
    use crate::video::ui::video_shell::playback::{is_remote_uri, seek_fraction};

    #[test]
    fn seek_fraction_none_without_known_duration() {
        assert_eq!(seek_fraction(5.0, 0.0), None);
        assert_eq!(seek_fraction(5.0, -1.0), None);
    }

    #[test]
    fn seek_fraction_maps_and_clamps() {
        assert_eq!(seek_fraction(0.0, 100.0), Some(0.0));
        assert_eq!(seek_fraction(50.0, 100.0), Some(0.5));
        assert_eq!(seek_fraction(100.0, 100.0), Some(1.0));
        // Position past the end clamps to 1.0; before start clamps to 0.0.
        assert_eq!(seek_fraction(150.0, 100.0), Some(1.0));
        assert_eq!(seek_fraction(-10.0, 100.0), Some(0.0));
    }

    #[test]
    fn remote_uri_detects_streaming_and_disc_schemes() {
        for uri in [
            "http://h/v.mp4",
            "https://h/v.mp4",
            "rtsp://h/s",
            "rtmp://h/s",
            "dvd://1",
            "bd://2",
            "  https://trimmed ",
        ] {
            assert!(is_remote_uri(uri), "{uri} should be remote");
        }
    }

    #[test]
    fn remote_uri_rejects_local_paths() {
        for path in ["/home/u/v.mkv", "file:///v.mp4", "video.mp4", "ftp://h/x"] {
            assert!(!is_remote_uri(path), "{path} should be local");
        }
    }
}

mod resolve_open_files_tests {
    use crate::video::ui::video_shell::playback::resolve_open_files;
    #[cfg(not(miri))]
    use std::fs;
    #[cfg(not(miri))]
    use std::path::Path;

    #[cfg(not(miri))]
    fn touch(dir: &Path, name: &str) -> String {
        let path = dir.join(name);
        fs::write(&path, b"x").unwrap();
        path.to_string_lossy().into_owned()
    }

    fn resolve_by_name(paths: Vec<String>) -> Option<(Vec<String>, usize, Option<String>)> {
        resolve_open_files(paths, &("name".to_string(), false))
    }

    #[test]
    fn empty_input_resolves_to_nothing() {
        assert_eq!(resolve_by_name(vec![]), None);
    }

    #[test]
    fn remote_uri_passes_through() {
        // Not a playlist, not local → the bare entry, no browse context.
        assert_eq!(
            resolve_by_name(vec!["rtsp://host/stream".to_string()]),
            Some((vec!["rtsp://host/stream".to_string()], 0, None))
        );
    }

    #[test]
    fn hls_stream_plays_directly() {
        for url in [
            "http://host/live/u/p/560.m3u8",
            "HTTPS://host/list.m3u",
            "udp://@239.0.0.1:1234",
        ] {
            assert_eq!(
                resolve_by_name(vec![url.to_string()]),
                Some((vec![url.to_string()], 0, None)),
                "{url}"
            );
        }
    }

    #[test]
    fn multiple_inputs_keep_media_order() {
        let out = resolve_by_name(vec![
            "/x/B.mp4".to_string(),
            "/x/a.mkv".to_string(),
            "/x/notes.txt".to_string(),
        ]);
        assert_eq!(
            out,
            Some((
                vec!["/x/B.mp4".to_string(), "/x/a.mkv".to_string()],
                0,
                None
            ))
        );
    }

    #[test]
    fn multiple_non_media_inputs_resolve_to_nothing() {
        assert_eq!(
            resolve_by_name(vec!["/x/a.txt".to_string(), "/x/b.doc".to_string()]),
            None
        );
    }

    #[cfg(not(miri))]
    #[test]
    fn local_file_queues_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.mp4");
        let opened = touch(dir.path(), "b.mp4");
        touch(dir.path(), "c.mp4");
        let (folder, pos, context) = resolve_by_name(vec![opened.clone()]).unwrap();
        assert_eq!(folder.len(), 3, "all siblings scanned: {folder:?}");
        // Position must index the opened file specifically (sorted a,b,c → 1).
        assert_eq!(pos, 1);
        assert_eq!(folder[pos], opened);
        assert_eq!(
            context.as_deref(),
            Some(dir.path().to_string_lossy().as_ref())
        );
    }

    #[cfg(not(miri))]
    #[test]
    fn unlisted_file_plays_alone() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.mp4");
        // "Open With" offers every type the desktop entry claims; one whose
        // extension the scan lacks must not start the folder's first video.
        let opened = touch(dir.path(), "b.unknownvideo");
        let (entries, pos, context) = resolve_by_name(vec![opened.clone()]).unwrap();
        assert_eq!((entries, pos, context), (vec![opened], 0, None));
    }

    #[cfg(not(miri))]
    #[test]
    fn single_playlist_returns_its_entries_without_context() {
        let dir = tempfile::tempdir().unwrap();
        let a = touch(dir.path(), "a.mp4");
        let b = touch(dir.path(), "b.mp4");
        let playlist = dir.path().join("list.m3u8");
        fs::write(&playlist, format!("#EXTM3U\n{a}\n{b}\n")).unwrap();
        let (entries, pos, context) =
            resolve_by_name(vec![playlist.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(entries.len(), 2, "both playlist entries kept: {entries:?}");
        assert_eq!(pos, 0);
        assert_eq!(context, None);
    }

    #[cfg(not(miri))]
    #[test]
    fn single_empty_playlist_resolves_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let playlist = dir.path().join("empty.m3u8");
        fs::write(&playlist, "#EXTM3U\n").unwrap();
        assert_eq!(
            resolve_by_name(vec![playlist.to_string_lossy().into_owned()]),
            None
        );
    }
}

mod dispatch_logic_tests {
    use crate::video::ui::video_shell::playback::{
        is_same_playlist, playlist_index_of, seek_target_secs,
    };

    #[test]
    fn seek_target_none_until_duration_known() {
        assert_eq!(seek_target_secs(0.5, 0.0), None);
        assert_eq!(seek_target_secs(0.5, -1.0), None);
    }

    #[test]
    fn seek_target_scales_fraction_by_duration() {
        assert_eq!(seek_target_secs(0.0, 200.0), Some(0.0));
        assert_eq!(seek_target_secs(0.25, 200.0), Some(50.0));
        assert_eq!(seek_target_secs(1.0, 200.0), Some(200.0));
    }

    #[test]
    fn same_playlist_compares_full_lists() {
        let a = vec!["x".to_string(), "y".to_string()];
        assert!(is_same_playlist(&a, &a.clone()));
        assert!(!is_same_playlist(&a, &["x".to_string()]));
        assert!(!is_same_playlist(&a, &["y".to_string(), "x".to_string()]));
    }

    #[test]
    fn playlist_index_locates_only_exact_matches() {
        let paths = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(playlist_index_of(&paths, "a"), Some(0));
        assert_eq!(playlist_index_of(&paths, "c"), Some(2));
        assert_eq!(playlist_index_of(&paths, "z"), None);
        assert_eq!(playlist_index_of(&[], "a"), None);
    }
}
