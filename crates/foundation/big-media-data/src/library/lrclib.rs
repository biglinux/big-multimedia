//! LRCLIB API client: free, no API key, open-source lyrics database.
//! <https://lrclib.net/docs>

use big_os_kit::http_client::http_get_with_retry;
use big_os_kit::url::encode_query_value as encode_url;

/// Synced lyrics preferred, plain lyrics fallback.
fn parse_lyrics_json(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json.get("syncedLyrics")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            json.get("plainLyrics")
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .map(String::from)
}

/// Blocking; call from a worker thread. Synced LRC preferred, plain fallback.
pub fn fetch_lyrics(artist: &str, title: &str, album: Option<&str>) -> Option<String> {
    let mut url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        encode_url(artist),
        encode_url(title),
    );
    if let Some(alb) = album {
        url.push_str("&album_name=");
        url.push_str(&encode_url(alb));
    }

    let body = http_get_with_retry(&url)?;
    parse_lyrics_json(&body)
}

fn parse_search_results_json(body: &str) -> Vec<(String, String, String)> {
    let arr: Vec<serde_json::Value> = match serde_json::from_str(body) {
        Ok(a) => a,
        Err(_) => return Vec::new(),
    };

    arr.iter()
        .filter_map(|obj| {
            let artist = obj.get("artistName")?.as_str()?;
            let title = obj.get("trackName")?.as_str()?;
            let lyrics = obj
                .get("syncedLyrics")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    obj.get("plainLyrics")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                })?;
            Some((artist.to_string(), title.to_string(), lyrics.to_string()))
        })
        .collect()
}

/// Blocking. Returns `(artist, title, lyrics)` tuples.
pub fn search_lyrics(query: &str) -> Vec<(String, String, String)> {
    let url = format!("https://lrclib.net/api/search?q={}", encode_url(query));

    let Some(body) = http_get_with_retry(&url) else {
        return Vec::new();
    };
    parse_search_results_json(&body)
}

/// Last-resort fallback (plain text only). Blocking.
pub fn fetch_lyrics_ovh(artist: &str, title: &str) -> Option<String> {
    let url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        encode_url(artist),
        encode_url(title),
    );

    let body = http_get_with_retry(&url)?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;

    json.get("lyrics")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Cascade: LRCLIB exact (with album) → LRCLIB exact (no album, since album
/// metadata is often wrong) → LRCLIB free-text → lyrics.ovh. Blocking.
pub fn fetch_lyrics_with_fallback(
    artist: &str,
    title: &str,
    album: Option<&str>,
) -> Option<String> {
    if album.is_some()
        && let Some(lyrics) = fetch_lyrics(artist, title, album)
    {
        return Some(lyrics);
    }
    if let Some(lyrics) = fetch_lyrics(artist, title, None) {
        return Some(lyrics);
    }
    let query = format!("{artist} {title}");
    let results = search_lyrics(&query);
    if let Some((_a, _t, lyrics)) = results.into_iter().next() {
        return Some(lyrics);
    }
    fetch_lyrics_ovh(artist, title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_url_ascii_passthrough() {
        assert_eq!(encode_url("hello"), "hello");
        assert_eq!(encode_url("Hello123"), "Hello123");
    }

    #[test]
    fn encode_url_unreserved_chars() {
        assert_eq!(encode_url("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn encode_url_spaces_and_special() {
        assert_eq!(encode_url("hello world"), "hello%20world");
        assert_eq!(encode_url("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn encode_url_unicode() {
        let encoded = encode_url("café");
        assert_eq!(encoded, "caf%C3%A9");
    }

    #[test]
    fn encode_url_empty() {
        assert_eq!(encode_url(""), "");
    }

    #[test]
    fn encode_url_quotes() {
        assert_eq!(encode_url("\"quoted\""), "%22quoted%22");
    }

    #[test]
    fn parse_lyrics_prefers_synced() {
        let json = r#"{"syncedLyrics":"[00:01]Hello","plainLyrics":"Hello"}"#;
        assert_eq!(parse_lyrics_json(json), Some("[00:01]Hello".to_string()));
    }

    #[test]
    fn parse_lyrics_falls_back_to_plain() {
        let json = r#"{"syncedLyrics":"","plainLyrics":"Hello world"}"#;
        assert_eq!(parse_lyrics_json(json), Some("Hello world".to_string()));
    }

    #[test]
    fn parse_lyrics_null_synced() {
        let json = r#"{"syncedLyrics":null,"plainLyrics":"Just plain"}"#;
        assert_eq!(parse_lyrics_json(json), Some("Just plain".to_string()));
    }

    #[test]
    fn parse_lyrics_both_empty() {
        let json = r#"{"syncedLyrics":"","plainLyrics":""}"#;
        assert_eq!(parse_lyrics_json(json), None);
    }

    #[test]
    fn parse_lyrics_missing_keys() {
        let json = r#"{"other":"data"}"#;
        assert_eq!(parse_lyrics_json(json), None);
    }

    #[test]
    fn parse_lyrics_invalid_json() {
        assert_eq!(parse_lyrics_json("not json"), None);
    }

    #[test]
    fn parse_search_results_valid() {
        let json = r#"[
            {"artistName":"Artist","trackName":"Song","syncedLyrics":"[00:01]Hi","plainLyrics":"Hi"}
        ]"#;
        let results = parse_search_results_json(json);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "Artist");
        assert_eq!(results[0].1, "Song");
        assert_eq!(results[0].2, "[00:01]Hi");
    }

    #[test]
    fn parse_search_prefers_synced() {
        let json =
            r#"[{"artistName":"A","trackName":"B","syncedLyrics":"synced","plainLyrics":"plain"}]"#;
        let results = parse_search_results_json(json);
        assert_eq!(results[0].2, "synced");
    }

    #[test]
    fn parse_search_falls_back_to_plain() {
        let json =
            r#"[{"artistName":"A","trackName":"B","syncedLyrics":"","plainLyrics":"plain only"}]"#;
        let results = parse_search_results_json(json);
        assert_eq!(results[0].2, "plain only");
    }

    #[test]
    fn parse_search_skips_no_lyrics() {
        let json = r#"[{"artistName":"A","trackName":"B","syncedLyrics":"","plainLyrics":""}]"#;
        let results = parse_search_results_json(json);
        assert!(results.is_empty());
    }

    #[test]
    fn parse_search_empty_array() {
        assert!(parse_search_results_json("[]").is_empty());
    }

    #[test]
    fn parse_search_invalid_json() {
        assert!(parse_search_results_json("bad").is_empty());
    }

    #[test]
    fn parse_search_missing_fields() {
        let json = r#"[{"artistName":"A"}]"#;
        assert!(parse_search_results_json(json).is_empty());
    }
}
