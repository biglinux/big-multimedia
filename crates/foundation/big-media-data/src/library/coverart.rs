//! Cover Art Archive fallback — fetch album art from MusicBrainz + CAA.
//! Free API, no key needed. Rate limit: 1 req/sec (enforced via User-Agent).
//! Data is CC0 licensed.

use big_os_kit::http_client::{RequestHeaders, http_get_bytes_capped, http_get_json};
use big_os_kit::url::encode_query_value as url_encode;

const MB_SEARCH: &str = "https://musicbrainz.org/ws/2/release";

fn build_mb_search_url(artist: &str, album: &str) -> String {
    let query = format!(
        "artist:\"{}\" AND release:\"{}\"",
        artist.replace('"', ""),
        album.replace('"', ""),
    );
    format!(
        "{}?query={}&fmt=json&limit=1",
        MB_SEARCH,
        url_encode(&query),
    )
}

fn extract_release_mbid(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json.get("releases")?
        .as_array()?
        .first()?
        .get("id")?
        .as_str()
        .map(String::from)
}

fn build_caa_url(mbid: &str) -> String {
    format!("https://coverartarchive.org/release/{mbid}/front-250")
}

/// Blocking — call from a background thread.
pub fn fetch_cover_art(artist: &str, album: &str) -> Option<Vec<u8>> {
    let body = http_get_json(&build_mb_search_url(artist, album))?;
    let mbid = extract_release_mbid(&body)?;

    // MusicBrainz: 1 req/sec — pace before the next call.
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let caa_url = build_caa_url(&mbid);

    let cover_art_response = http_get_bytes_capped(
        &caa_url,
        &RequestHeaders::default(),
        5 * 1024 * 1024,
        std::time::Duration::from_secs(15),
    )
    .ok()?;

    if cover_art_response.status != 200 {
        return None;
    }

    if cover_art_response.bytes.is_empty() {
        None
    } else {
        Some(cover_art_response.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_url_strips_quotes() {
        let url = build_mb_search_url("The \"Beatles\"", "Let \"It\" Be");
        assert!(!url.contains('"'));
        assert!(url.contains("artist"));
        assert!(url.contains("release"));
    }

    #[test]
    fn search_url_encodes_spaces() {
        let url = build_mb_search_url("Pink Floyd", "Dark Side");
        assert!(url.contains("%20"));
    }

    #[test]
    fn search_url_empty_inputs() {
        let url = build_mb_search_url("", "");
        assert!(url.starts_with(MB_SEARCH));
    }

    #[test]
    fn extract_mbid_valid_json() {
        let json = r#"{"releases":[{"id":"abc-123","title":"Test"}]}"#;
        assert_eq!(extract_release_mbid(json), Some("abc-123".to_string()));
    }

    #[test]
    fn extract_mbid_empty_releases() {
        let json = r#"{"releases":[]}"#;
        assert_eq!(extract_release_mbid(json), None);
    }

    #[test]
    fn extract_mbid_missing_id() {
        let json = r#"{"releases":[{"title":"No ID"}]}"#;
        assert_eq!(extract_release_mbid(json), None);
    }

    #[test]
    fn extract_mbid_invalid_json() {
        assert_eq!(extract_release_mbid("not json"), None);
    }

    #[test]
    fn extract_mbid_no_releases_key() {
        let json = r#"{"other": "data"}"#;
        assert_eq!(extract_release_mbid(json), None);
    }

    #[test]
    fn caa_url_format() {
        let url = build_caa_url("abc-123");
        assert_eq!(url, "https://coverartarchive.org/release/abc-123/front-250");
    }
}
