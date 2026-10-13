//! MusicBrainz metadata lookup: fetch genre, year, and track number for recordings.
//! Free API, no key needed; it allows one request per second per client, and
//! lookups here run only on an explicit user action.

use big_os_kit::http_client::http_get_json;
use big_os_kit::url::encode_query_value as url_encode;

const MB_API: &str = "https://musicbrainz.org/ws/2";
const MIN_MATCH_SCORE: i64 = 80;

#[derive(Debug, Default)]
pub struct MbMetadata {
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<i32>,
}

/// Blocking; call from a background thread.
pub fn lookup_metadata(artist: &str, title: &str) -> Option<MbMetadata> {
    let query = format!(
        "artist:\"{}\" AND recording:\"{}\"",
        artist.replace('"', ""),
        title.replace('"', ""),
    );
    let url = format!(
        "{}/recording?query={}&fmt=json&limit=1",
        MB_API,
        url_encode(&query),
    );

    let body = http_get_json(&url)?;
    parse_recording_json(&body)
}

fn parse_recording_json(body: &str) -> Option<MbMetadata> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let recording = json.get("recordings")?.as_array()?.first()?;

    // Reject low-confidence matches: MB otherwise returns the best of a bad bunch.
    let score = recording
        .get("score")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    if score < MIN_MATCH_SCORE {
        return None;
    }

    let mut meta = MbMetadata::default();
    if let Some(genre) = extract_best_tag(recording) {
        meta.genre = Some(genre);
    }
    if let Some(release) = recording
        .get("releases")
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())
    {
        populate_release_fields(release, &mut meta);
    }
    Some(meta)
}

/// Highest-vote tag wins: MB tags are crowd-sourced, count reflects consensus.
fn extract_best_tag(recording: &serde_json::Value) -> Option<String> {
    let tags = recording.get("tags")?.as_array()?;
    let best = tags.iter().max_by_key(|t| {
        t.get("count")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
    })?;
    let name = best.get("name")?.as_str()?;
    Some(capitalize(name))
}

fn populate_release_fields(release: &serde_json::Value, meta: &mut MbMetadata) {
    if let Some(year) = release
        .get("date")
        .and_then(|d| d.as_str())
        .and_then(|d| d.split('-').next())
        .and_then(|year_str| year_str.parse::<i32>().ok())
    {
        meta.year = Some(year);
    }
    meta.track_number = extract_first_track_number(release);
}

fn extract_first_track_number(release: &serde_json::Value) -> Option<i32> {
    release.get("media")?.as_array()?.iter().find_map(|medium| {
        medium
            .get("track-list")?
            .as_array()?
            .first()?
            .get("number")?
            .as_str()?
            .parse::<i32>()
            .ok()
    })
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalize_lowercase() {
        assert_eq!(capitalize("rock"), "Rock");
    }

    #[test]
    fn capitalize_already_upper() {
        assert_eq!(capitalize("Rock"), "Rock");
    }

    #[test]
    fn capitalize_empty() {
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn capitalize_single_char() {
        assert_eq!(capitalize("x"), "X");
    }

    #[test]
    fn capitalize_unicode() {
        assert_eq!(capitalize("ñoño"), "Ñoño");
    }

    fn make_recording_json(score: i64, extras: &str) -> String {
        format!(r#"{{"recordings":[{{"score":{score},"title":"Test"{extras}}}]}}"#)
    }

    #[test]
    fn parse_recording_low_score_rejected() {
        let json = make_recording_json(50, "");
        assert!(parse_recording_json(&json).is_none());
    }

    #[test]
    fn parse_recording_exact_threshold() {
        let json = make_recording_json(80, "");
        assert!(parse_recording_json(&json).is_some());
    }

    #[test]
    fn parse_recording_extracts_genre() {
        let json = make_recording_json(
            95,
            r#","tags":[{"name":"rock","count":5},{"name":"pop","count":2}]"#,
        );
        let meta = parse_recording_json(&json).unwrap();
        assert_eq!(meta.genre, Some("Rock".to_string()));
    }

    #[test]
    fn parse_recording_extracts_year() {
        let json = make_recording_json(
            90,
            r#","releases":[{"date":"1975-11-21","title":"Test Album","id":"rel-1"}]"#,
        );
        let meta = parse_recording_json(&json).unwrap();
        assert_eq!(meta.year, Some(1975));
    }

    #[test]
    fn parse_recording_extracts_track_number() {
        let json = make_recording_json(
            90,
            r#","releases":[{"id":"r1","title":"A","media":[{"track-list":[{"number":"3"}]}]}]"#,
        );
        let meta = parse_recording_json(&json).unwrap();
        assert_eq!(meta.track_number, Some(3));
    }

    #[test]
    fn parse_recording_empty_recordings() {
        let json = r#"{"recordings":[]}"#;
        assert!(parse_recording_json(json).is_none());
    }

    #[test]
    fn parse_recording_invalid_json() {
        assert!(parse_recording_json("not json").is_none());
    }

    #[test]
    fn parse_recording_no_recordings_key() {
        let json = r#"{"other":"data"}"#;
        assert!(parse_recording_json(json).is_none());
    }

    #[test]
    fn parse_recording_partial_date() {
        let json =
            make_recording_json(90, r#","releases":[{"id":"r1","title":"A","date":"2020"}]"#);
        let meta = parse_recording_json(&json).unwrap();
        assert_eq!(meta.year, Some(2020));
    }
}
