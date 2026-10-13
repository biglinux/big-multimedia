//! Radio Browser API JSON -> station parser; spam-vote filter
//! (>= 50k votes drops bot-rigged stations); locale-based country detection.
//!
//! This module never opens network sockets: callers fetch the JSON
//! payload themselves and pass it to `parse_stations_json` or
//! `parse_stations_json_filtered`.

use log::warn;

/// One station row as `(name, url, country, tags, favicon)`.
///
/// Returned by `parse_stations_json` and `parse_stations_json_filtered`;
/// trimmed of surrounding whitespace and validated to reject empty
/// names/URLs and non-HTTP favicons.
pub type RadioBrowserEntry = (String, String, String, String, String);

const SPAM_VOTE_THRESHOLD: u64 = 50_000;
const _: () = assert!(SPAM_VOTE_THRESHOLD > 0);
const _: () = assert!(SPAM_VOTE_THRESHOLD <= 100_000);

/// Parse the Radio Browser station JSON `body` into [`RadioBrowserEntry`] rows.
///
/// Returns an empty vector when `body` is not valid JSON or has no usable
/// stations. Stations with empty `name` or `url` after trimming are
/// dropped. Favicons that are missing, equal to `"null"`, or use a
/// non-HTTP scheme are normalized to an empty string.
#[must_use]
pub fn parse_stations_json(body: &str) -> Vec<RadioBrowserEntry> {
    parse_stations_inner(body, false)
}

/// Like [`parse_stations_json`] but drops stations with `votes >= 50_000`.
///
/// Radio Browser hosts community-voted stations; voting can be gamed, so a
/// large vote count is a stronger signal of bot-rigged spam than of
/// quality. This filter is used for the "discover" view to drop rigged
/// stations while [`parse_stations_json`] keeps everything for the user's
/// own searches.
#[must_use]
pub fn parse_stations_json_filtered(body: &str) -> Vec<RadioBrowserEntry> {
    parse_stations_inner(body, true)
}

fn parse_stations_inner(body: &str, filter_spam: bool) -> Vec<RadioBrowserEntry> {
    let stations: Vec<serde_json::Value> = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(error) => {
            warn!("[radio] JSON parse fail: {error}");
            return Vec::new();
        }
    };

    stations
        .iter()
        .filter_map(|station| {
            if filter_spam {
                let votes = station
                    .get("votes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                if votes >= SPAM_VOTE_THRESHOLD {
                    return None;
                }
            }

            let name = station.get("name")?.as_str()?.trim().to_string();
            let url = station
                .get("url_resolved")
                .or_else(|| station.get("url"))?
                .as_str()?
                .trim()
                .to_string();
            if name.is_empty() || url.is_empty() {
                return None;
            }

            let country = station
                .get("country")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let tags = station
                .get("tags")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let favicon = station
                .get("favicon")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != "null" && value.starts_with("http"))
                .unwrap_or("")
                .to_string();

            Some((name, url, country, tags, favicon))
        })
        .collect()
}

/// Best-effort detection of the user's ISO 3166-1 alpha-2 country code from locale env vars.
///
/// Inspects `LC_ALL`, `LANG`, and `LANGUAGE` in order, splits on `_` and
/// `.`, and returns the uppercase two-letter country tail when present.
/// Returns an empty string when no usable code is found (for example,
/// under the `C` locale).
#[must_use]
pub fn detect_country_code() -> String {
    for var in ["LC_ALL", "LANG", "LANGUAGE"] {
        if let Ok(locale) = std::env::var(var)
            && let Some(country_code) = country_code_from_locale(&locale)
        {
            return country_code;
        }
    }
    String::new()
}

fn country_code_from_locale(locale: &str) -> Option<String> {
    let locale = locale.split('.').next().unwrap_or(locale);
    let country_code = locale.split('_').nth(1)?.to_uppercase();
    if country_code.len() == 2
        && country_code
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        Some(country_code)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_code_from_locale_all_cases() {
        assert_eq!(
            country_code_from_locale("pt_BR.UTF-8").as_deref(),
            Some("BR")
        );
        assert_eq!(
            country_code_from_locale("en_US.UTF-8").as_deref(),
            Some("US")
        );
        assert_eq!(country_code_from_locale("de_DE").as_deref(), Some("DE"));
        assert_eq!(country_code_from_locale("C"), None);
        assert_eq!(country_code_from_locale("pt_BRA"), None);
    }

    #[test]
    fn parse_valid_stations() {
        let json = r#"[
            {
                "name": "Radio Foo",
                "url": "http://stream.foo.com/live",
                "url_resolved": "http://stream.foo.com/live.m3u",
                "country": "Brazil",
                "tags": "pop,rock",
                "favicon": "https://foo.com/icon.png"
            },
            {
                "name": "Jazz FM",
                "url": "http://jazz.example.com/stream",
                "country": "US",
                "tags": "jazz",
                "favicon": ""
            }
        ]"#;
        let results = parse_stations_json(json);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "Radio Foo");
        assert_eq!(results[0].1, "http://stream.foo.com/live.m3u");
        assert_eq!(results[0].2, "Brazil");
        assert_eq!(results[0].3, "pop,rock");
        assert_eq!(results[0].4, "https://foo.com/icon.png");
        assert_eq!(results[1].1, "http://jazz.example.com/stream");
        assert_eq!(results[1].4, "");
    }

    #[test]
    fn parse_filters_empty_name_or_url() {
        let json = r#"[
            {"name": "", "url": "http://a.com"},
            {"name": "Good", "url": ""},
            {"name": "  ", "url": "http://b.com"},
            {"name": "Valid", "url": "http://c.com"}
        ]"#;
        let results = parse_stations_json(json);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "Valid");
    }

    #[test]
    fn parse_empty_array() {
        assert!(parse_stations_json("[]").is_empty());
    }

    #[test]
    fn parse_invalid_json_returns_empty() {
        assert!(parse_stations_json("not json at all").is_empty());
    }

    #[test]
    fn parse_null_favicon_rejected() {
        let json = r#"[{"name": "Test", "url": "http://test.com", "favicon": "null"}]"#;
        let results = parse_stations_json(json);
        assert_eq!(results[0].4, "");
    }

    #[test]
    fn parse_non_http_favicon_rejected() {
        let json = r#"[{"name": "Test", "url": "http://test.com", "favicon": "ftp://icon.png"}]"#;
        let results = parse_stations_json(json);
        assert_eq!(results[0].4, "");
    }

    #[test]
    fn parse_trims_whitespace() {
        let json = r#"[{"name": "  Spaced  ", "url": "  http://x.com  ", "favicon": "  https://icon.png  "}]"#;
        let results = parse_stations_json(json);
        assert_eq!(results[0].0, "Spaced");
        assert_eq!(results[0].1, "http://x.com");
        assert_eq!(results[0].4, "https://icon.png");
    }

    #[test]
    fn parse_missing_optional_fields() {
        let json = r#"[{"name": "Bare", "url": "http://bare.com"}]"#;
        let results = parse_stations_json(json);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].2, "");
        assert_eq!(results[0].3, "");
        assert_eq!(results[0].4, "");
    }

    #[test]
    fn filtered_parser_removes_high_vote_stations() {
        let json = r#"[
            {"name": "Legit Station", "url": "http://legit.com", "votes": 1000},
            {"name": "Bot Station", "url": "http://bot.com", "votes": 999999},
            {"name": "Normal FM", "url": "http://normal.com", "votes": 49999}
        ]"#;
        let results = parse_stations_json_filtered(json);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "Legit Station");
        assert_eq!(results[1].0, "Normal FM");
    }

    #[test]
    fn filtered_parser_keeps_stations_without_votes_field() {
        let json = r#"[{"name": "No Votes", "url": "http://x.com"}]"#;
        let results = parse_stations_json_filtered(json);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn unfiltered_parser_keeps_all_votes() {
        let json = r#"[{"name": "Bot Station", "url": "http://bot.com", "votes": 999999}]"#;
        let results = parse_stations_json(json);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn spam_vote_threshold_is_compile_time_checked() {}
}
