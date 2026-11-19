//! Fuzz target bodies, one per parser of outside bytes; each `fuzz_targets/`
//! adapter calls one of them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use big_app_kit_core::remote_control::{RemoteHandlers, StatusSnapshot, fuzz_respond};
use big_media_runtime::media::iptv::{
    MAX_PLAYLIST_ENTRIES, decode_playlist, parse_channel_list, parse_m3u_content,
};
use big_os_kit::url::{
    ListOrigin, accepts_list_entry, file_uri_path, filename_from_url_path, is_http_url, redact_url,
};

const TOKEN: &str = "fuzz-token-0123456789";

/// A LAN request reaches a playback action only through `POST /<token>/`, and
/// no reply carries the token unless the request did.
pub fn remote_request(data: &[u8]) {
    let fired = Arc::new(AtomicBool::new(false));
    let action = |fired: &Arc<AtomicBool>| {
        let fired = Arc::clone(fired);
        Arc::new(move || {
            fired.store(true, Ordering::Relaxed);
            true
        })
    };
    let volume_fired = Arc::clone(&fired);
    let handlers = RemoteHandlers {
        play_pause: action(&fired),
        next: action(&fired),
        prev: action(&fired),
        set_volume: Arc::new(move |_| {
            volume_fired.store(true, Ordering::Relaxed);
            true
        }),
        status: Arc::new(|| StatusSnapshot {
            playing: false,
            title: String::new(),
            artist: String::new(),
            position_s: 0.0,
            duration_s: 0.0,
            volume: 50.0,
        }),
    };
    let reply = fuzz_respond(data, TOKEN, &handlers);
    assert!(
        reply.starts_with(b"HTTP/1.1 "),
        "reply without a status line"
    );
    let has_token = |bytes: &[u8]| bytes.windows(TOKEN.len()).any(|w| w == TOKEN.as_bytes());
    assert!(
        !has_token(&reply) || has_token(data),
        "reply leaked the token"
    );
    if fired.load(Ordering::Relaxed) {
        let route = format!("POST /{TOKEN}/");
        assert!(
            data.starts_with(route.as_bytes()),
            "action ran off the token route"
        );
    }
}

/// curl's header blocks never panic the reader.
pub fn http_headers(data: &[u8]) {
    big_os_kit::http_client::fuzz_read_headers(data);
}

/// Every episode a feed keeps has an address a network list may name.
pub fn podcast_feed(data: &[u8]) {
    let Ok(xml) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(feed) = big_media_data::library::podcast::parse_feed(xml) {
        for episode in &feed.episodes {
            assert!(
                accepts_list_entry(&episode.audio_url, ListOrigin::Network),
                "kept episode address {:?}",
                episode.audio_url
            );
        }
    }
}

/// Kept stations have a name and an HTTP stream; favicons are HTTP or empty;
/// the spam filter only removes.
pub fn radio_stations(data: &[u8]) {
    let Ok(body) = std::str::from_utf8(data) else {
        return;
    };
    let all = big_media_engines::radio_browser::parse_stations_json(body);
    for (name, url, _, _, favicon) in &all {
        assert!(!name.is_empty(), "station without a name");
        assert!(is_http_url(url), "station stream {url:?}");
        assert!(
            favicon.is_empty() || is_http_url(favicon),
            "favicon {favicon:?}"
        );
    }
    let filtered = big_media_engines::radio_browser::parse_stations_json_filtered(body);
    assert!(
        filtered.iter().all(|station| all.contains(station)),
        "filter added a station"
    );
}

/// An IPTV list stays under the entry cap, and the channel list keeps only
/// addresses a network list may name, counting the rest.
pub fn iptv_playlist(data: &[u8]) {
    let text = decode_playlist(data.to_vec());
    let parsed = parse_m3u_content(&text);
    assert!(
        parsed.entries.len() <= MAX_PLAYLIST_ENTRIES,
        "entry cap exceeded"
    );
    let channels = parse_channel_list(&text, ListOrigin::Network);
    assert_eq!(
        channels.entries.len() + channels.skipped,
        parsed.entries.len()
    );
    for channel in &channels.entries {
        assert!(
            accepts_list_entry(&channel.url, ListOrigin::Network),
            "kept {:?}",
            channel.url
        );
    }
}

/// A playlist file (PLS when the first byte is odd, else M3U) never panics the
/// reader and stays under the entry cap.
pub fn playlist_file(data: &[u8]) {
    let Some((&kind, content)) = data.split_first() else {
        return;
    };
    let extension = if kind % 2 == 1 { "pls" } else { "m3u" };
    let path = std::env::temp_dir().join(format!(
        "big-multimedia-fuzz-{}.{extension}",
        std::process::id()
    ));
    std::fs::write(&path, content).expect("write the playlist file");
    if let Ok(playlist) = big_media_runtime::media::playlist_scan::parse_playlist_file(&path) {
        assert!(
            playlist.entries.len() <= MAX_PLAYLIST_ENTRIES,
            "entry cap exceeded"
        );
    }
    std::fs::remove_file(&path).expect("remove the playlist file");
}

/// URL helpers keep their documented shapes on any string.
pub fn url_parsing(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(path) = file_uri_path(text) {
        assert!(path.is_absolute(), "file URI path {path:?}");
    }
    let name = filename_from_url_path(text, "fallback.bin");
    assert!(
        !name.is_empty() && !name.contains('/') && name.len() <= 200,
        "file name {name:?}"
    );
    let redacted = redact_url(text);
    if text.contains('#') {
        assert!(redacted.ends_with("#***"), "fragment kept in {redacted:?}");
    }
}
