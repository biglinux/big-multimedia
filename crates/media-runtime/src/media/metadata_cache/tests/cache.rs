use super::super::cache::parse_meta_cache;

/// Oracle: cache files with absent fields for unknown values and with nulls,
/// both written by released versions, plus one entry of the wrong type.
#[test]
fn cache_file_entries_read_alone() {
    let cache = parse_meta_cache(
        br#"{
            "/v/a.mkv": {"mtime": 7, "duration": 61.5, "resolution": "1920x1080",
                         "codec": "H.264", "audio_tracks": 2},
            "/v/b.mkv": {"mtime": 9, "duration": null, "resolution": null, "codec": null,
                         "audio_tracks": 0, "subtitle_tracks": 1},
            "/v/c.mkv": {"mtime": "yesterday"}
        }"#,
    );
    assert_eq!(cache.len(), 2);
    let a = &cache["/v/a.mkv"];
    assert_eq!(a.mtime, 7);
    assert_eq!(a.meta.duration_secs, Some(61.5));
    assert_eq!(a.meta.resolution.as_deref(), Some("1920x1080"));
    assert_eq!(a.meta.codec.as_deref(), Some("H.264"));
    assert_eq!((a.meta.audio_tracks, a.meta.subtitle_tracks), (2, 0));
    let b = &cache["/v/b.mkv"];
    assert_eq!((b.mtime, b.meta.duration_secs), (9, None));
    assert_eq!(b.meta.subtitle_tracks, 1);
    assert!(parse_meta_cache(b"[]").is_empty());
}
