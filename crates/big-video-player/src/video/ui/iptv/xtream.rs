//! Minimal Xtream Codes API client.
//!
//! Xtream Codes is a de-facto commercial IPTV panel format used by most paid
//! providers. Its `player_api.php` answers in JSON: first the account (whether
//! the user name and password are right, whether the plan is active, which
//! stream formats it serves), then the live categories and channels. Only
//! live TV is read; VOD and series are a later-phase enhancement.
//!
//! The panel's `get.php` export would return everything as one M3U, but on a
//! paid plan that list carries every film and series too, tens of megabytes
//! that a player of live channels does not need. It stays the fallback for
//! panels without the JSON API.
//!
//! The channels are turned into M3U text, so the cache and the parser are the
//! same as for an M3U provider. Its stream URLs carry [`CREDENTIALS`] in place
//! of the account's user name and password, which [`stream_url`] puts back
//! when a channel plays. Engine-free helper; blocking, so call it from a
//! worker thread.

use std::fmt;

use big_os_kit::http_client::RequestHeaders;
use big_os_kit::url::encode_query_value as urlencode;
use serde::Deserialize;
use serde::de::{
    DeserializeOwned, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor,
};
use serde_json::Value;

use super::fetcher::{FetchError, fetch_text};
use crate::media::iptv::MAX_PLAYLIST_ENTRIES;

/// Largest account answer parsed into a JSON tree; a real one is a few
/// kilobytes.
const MAX_ACCOUNT_BYTES: usize = 1024 * 1024;

/// The path segment standing for `user/password` in a stored stream URL.
/// Braces are not valid in a URL path, so no panel URL holds it.
const CREDENTIALS: &str = "/{credentials}/";

/// `url` from a stored line-up with the account's credentials put back.
#[must_use]
pub(super) fn stream_url(url: &str, username: &str, password: &str) -> String {
    url.replacen(
        CREDENTIALS,
        &format!("/{}/{}/", urlencode(username), urlencode(password)),
        1,
    )
}

/// Whether `url` is a stored stream URL waiting for an account's credentials.
#[must_use]
pub(super) fn names_credentials(url: &str) -> bool {
    url.contains(CREDENTIALS)
}

/// Whether `url` carries `password` as written or URL-encoded.
#[must_use]
pub(super) fn shows_password(url: &str, password: &str) -> bool {
    !password.is_empty() && (url.contains(password) || url.contains(&urlencode(password)))
}

/// `m3u` with the account's `/user/password/` path segments, as the panel
/// writes them or URL-encoded, replaced by [`CREDENTIALS`]. An entry that
/// carries the password in its query cannot be rebuilt that way, and is left
/// out with the lines that describe it.
fn without_credentials(m3u: &str, username: &str, password: &str) -> String {
    let raw = format!("/{username}/{password}/");
    let encoded_password = urlencode(password);
    let encoded = format!("/{}/{encoded_password}/", urlencode(username));
    let m3u = m3u
        .replace(&raw, CREDENTIALS)
        .replace(&encoded, CREDENTIALS);
    let in_query = |line: &str| {
        line.split_once('?').is_some_and(|(_, query)| {
            query.split(['&', '#']).any(|field| {
                field.split_once('=').is_some_and(|(_, value)| {
                    !password.is_empty() && (value == password || value == encoded_password)
                })
            })
        })
    };
    let mut kept = String::with_capacity(m3u.len());
    let mut described = String::new();
    for line in m3u.lines() {
        if line.starts_with("#EXTINF") || (line.starts_with('#') && !described.is_empty()) {
            described.push_str(line);
            described.push('\n');
            continue;
        }
        if line.starts_with('#') || !in_query(line) {
            kept.push_str(&described);
            kept.push_str(line);
            kept.push('\n');
        }
        described.clear();
    }
    kept.push_str(&described);
    kept
}

/// The live line-up of a Xtream account as M3U text, without its credentials.
pub(super) fn fetch_xtream_m3u(
    server: &str,
    username: &str,
    password: &str,
    headers: &RequestHeaders<'_>,
) -> Result<String, FetchError> {
    let base = server.trim_end_matches('/');
    let api = player_api_url(base, username, password);
    let account = match fetch_text(&api, headers) {
        Ok(text) if text.len() <= MAX_ACCOUNT_BYTES => serde_json::from_str::<Value>(&text).ok(),
        Ok(_) | Err(FetchError::Status(404)) => None,
        Err(error) => return Err(error),
    };
    // No JSON API here: the panel's M3U export.
    let Some(account) = account else {
        let m3u = fetch_text(&get_php_url(base, username, password), headers)?;
        return Ok(without_credentials(&m3u, username, password));
    };
    let extension = check_account(&account)?;
    let categories = fetch_text(&format!("{api}&action=get_live_categories"), headers)?;
    let streams = fetch_text(&format!("{api}&action=get_live_streams"), headers)?;
    let categories = first_entries(&categories, |_: &Category| true)?;
    let streams = live_streams(&streams)?;
    let m3u = live_m3u(base, extension, &categories, &streams);
    if m3u.lines().count() <= 1 {
        return Err(FetchError::Empty);
    }
    Ok(m3u)
}

#[derive(Deserialize)]
struct Category {
    #[serde(default, deserialize_with = "panel_text")]
    category_id: String,
    #[serde(default, deserialize_with = "panel_text")]
    category_name: String,
}

#[derive(Deserialize)]
struct LiveStream {
    #[serde(default, deserialize_with = "panel_text")]
    name: String,
    #[serde(default, deserialize_with = "panel_text")]
    stream_id: String,
    #[serde(default, deserialize_with = "panel_text")]
    stream_icon: String,
    #[serde(default, deserialize_with = "panel_text")]
    epg_channel_id: String,
    #[serde(default, deserialize_with = "panel_text")]
    category_id: String,
}

/// The streams of a `get_live_streams` answer whose id is a number, the only
/// form a panel's stream URL takes.
fn live_streams(json: &str) -> Result<Vec<LiveStream>, FetchError> {
    first_entries(json, |stream: &LiveStream| {
        !stream.stream_id.is_empty() && stream.stream_id.bytes().all(|b| b.is_ascii_digit())
    })
}

/// The elements of a JSON array that pass `keep`, up to one past
/// [`MAX_PLAYLIST_ENTRIES`] so the M3U parser reports the list as truncated.
/// Later elements are skipped without being built.
fn first_entries<T: DeserializeOwned>(
    json: &str,
    keep: fn(&T) -> bool,
) -> Result<Vec<T>, FetchError> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let entries = FirstEntries { keep }
        .deserialize(&mut deserializer)
        .and_then(|entries| deserializer.end().map(|()| entries))
        .map_err(|_| FetchError::Empty)?;
    Ok(entries)
}

struct FirstEntries<T> {
    keep: fn(&T) -> bool,
}

impl<'de, T: DeserializeOwned> DeserializeSeed<'de> for FirstEntries<T> {
    type Value = Vec<T>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<T>, D::Error> {
        deserializer.deserialize_any(self)
    }
}

/// A panel with nothing to list may answer an object or `null`.
impl<'de, T: DeserializeOwned> Visitor<'de> for FirstEntries<T> {
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("an array")
    }

    fn visit_unit<E>(self) -> Result<Vec<T>, E> {
        Ok(Vec::new())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Vec<T>, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(Vec::new())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
        let mut entries = Vec::new();
        while entries.len() <= MAX_PLAYLIST_ENTRIES {
            // Read whole, so a record of another shape is skipped alone.
            let Some(value) = seq.next_element::<serde_json::Value>()? else {
                return Ok(entries);
            };
            if let Ok(entry) = T::deserialize(value)
                && (self.keep)(&entry)
            {
                entries.push(entry);
            }
        }
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(entries)
    }
}

/// A field the panel may send as a string or a number; anything else reads as
/// empty.
fn panel_text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    deserializer.deserialize_any(TextVisitor)
}

struct TextVisitor;

impl<'de> Visitor<'de> for TextVisitor {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string or a number")
    }

    fn visit_str<E>(self, value: &str) -> Result<String, E> {
        Ok(value.to_owned())
    }

    fn visit_i64<E>(self, value: i64) -> Result<String, E> {
        Ok(value.to_string())
    }

    fn visit_u64<E>(self, value: u64) -> Result<String, E> {
        Ok(value.to_string())
    }

    fn visit_f64<E>(self, value: f64) -> Result<String, E> {
        Ok(value.to_string())
    }

    fn visit_bool<E>(self, _: bool) -> Result<String, E> {
        Ok(String::new())
    }

    fn visit_unit<E>(self) -> Result<String, E> {
        Ok(String::new())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<String, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(String::new())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<String, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(String::new())
    }
}

/// `player_api.php` for the account (URL-encoding the credentials).
#[must_use]
fn player_api_url(base: &str, username: &str, password: &str) -> String {
    format!(
        "{base}/player_api.php?username={}&password={}",
        urlencode(username),
        urlencode(password)
    )
}

/// The panel's whole-account M3U export.
#[must_use]
pub(super) fn get_php_url(base: &str, username: &str, password: &str) -> String {
    let base = base.trim_end_matches('/');
    format!(
        "{base}/get.php?username={}&password={}&type=m3u_plus&output=m3u8",
        urlencode(username),
        urlencode(password)
    )
}

/// Whether the account may watch, and the stream format to ask for: HLS when
/// the plan serves it, MPEG-TS otherwise.
fn check_account(account: &Value) -> Result<&'static str, FetchError> {
    let user = &account["user_info"];
    // Panels send numbers and strings alike.
    let auth = match &user["auth"] {
        Value::Number(number) => number.as_i64() == Some(1),
        Value::String(text) => text == "1",
        Value::Bool(flag) => *flag,
        _ => false,
    };
    if !auth {
        return Err(FetchError::LoginRejected);
    }
    let status = user["status"].as_str().unwrap_or("Active");
    if !status.eq_ignore_ascii_case("active") {
        return Err(FetchError::AccountInactive(status.to_string()));
    }
    let serves_hls = user["allowed_output_formats"]
        .as_array()
        .is_some_and(|formats| formats.iter().any(|format| format == "m3u8"));
    Ok(if serves_hls { "m3u8" } else { "ts" })
}

/// `text` on one M3U line: a line break would start an entry of its own.
fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

/// An M3U attribute value: a double quote would end it early.
fn attribute(text: &str) -> String {
    one_line(&text.replace('"', "'"))
}

/// The live streams as M3U, grouped by their category names.
fn live_m3u(
    base: &str,
    extension: &str,
    categories: &[Category],
    streams: &[LiveStream],
) -> String {
    let names: std::collections::HashMap<&str, &str> = categories
        .iter()
        .map(|category| {
            (
                category.category_id.as_str(),
                category.category_name.as_str(),
            )
        })
        .collect();
    let base = one_line(base);
    let mut m3u = String::from("#EXTM3U\n");
    for stream in streams {
        let group = names
            .get(stream.category_id.as_str())
            .copied()
            .unwrap_or_default();
        m3u.push_str(&format!(
            "#EXTINF:-1 tvg-id=\"{}\" tvg-logo=\"{}\" group-title=\"{}\",{}\n{base}/live{CREDENTIALS}{}.{extension}\n",
            attribute(&stream.epg_channel_id),
            attribute(&stream.stream_icon),
            attribute(group),
            one_line(&stream.name),
            stream.stream_id,
        ));
    }
    m3u
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::iptv::parse_m3u_content;
    use serde_json::json;

    #[test]
    fn get_php_url_trims_and_encodes() {
        let url = get_php_url("http://srv.example/", "u@x", "p&w");
        assert_eq!(
            url,
            "http://srv.example/get.php?username=u%40x&password=p%26w&type=m3u_plus&output=m3u8"
        );
    }

    #[test]
    fn player_api_url_encodes_credentials() {
        assert_eq!(
            player_api_url("https://host.tld", "u@x", "p&w"),
            "https://host.tld/player_api.php?username=u%40x&password=p%26w"
        );
    }

    #[test]
    fn account_check_reads_the_panels_answer() {
        let active = json!({"user_info": {"auth": 1, "status": "Active",
            "allowed_output_formats": ["m3u8", "ts"]}});
        assert_eq!(check_account(&active), Ok("m3u8"));
        let ts_only = json!({"user_info": {"auth": "1", "status": "Active",
            "allowed_output_formats": ["ts"]}});
        assert_eq!(check_account(&ts_only), Ok("ts"));
        let wrong = json!({"user_info": {"auth": 0}});
        assert_eq!(check_account(&wrong), Err(FetchError::LoginRejected));
        let expired = json!({"user_info": {"auth": 1, "status": "Expired"}});
        assert_eq!(
            check_account(&expired),
            Err(FetchError::AccountInactive("Expired".into()))
        );
    }

    #[test]
    fn live_streams_become_grouped_channels() {
        let categories = r#"[
            {"category_id": "1", "category_name": "News"},
            {"category_id": 2, "category_name": "Sports \"HD\""}
        ]"#;
        let streams = r#"[
            {"name": "Globo, SP", "stream_id": 101, "stream_icon": "http://logo/1.png",
             "epg_channel_id": "globo.br", "category_id": "1", "tv_archive": [1, {"a": 2}]},
            {"name": "ESPN", "stream_id": "202", "stream_icon": null, "category_id": 2},
            {"name": "No id"}
        ]"#;
        let categories = first_entries(categories, |_: &Category| true).unwrap();
        let streams = live_streams(streams).unwrap();
        let m3u = live_m3u("http://srv", "m3u8", &categories, &streams);
        assert!(m3u.contains("tvg-id=\"globo.br\""));
        let channels = parse_m3u_content(&m3u).entries;
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Globo, SP");
        assert_eq!(channels[0].group, "News");
        assert_eq!(channels[0].logo, "http://logo/1.png");
        assert_eq!(channels[1].group, "Sports 'HD'");
        let urls: Vec<String> = channels
            .iter()
            .map(|channel| stream_url(&channel.url, "u@x", "p"))
            .collect();
        assert_eq!(
            urls,
            [
                "http://srv/live/u%40x/p/101.m3u8",
                "http://srv/live/u%40x/p/202.m3u8"
            ]
        );
    }

    #[test]
    fn invalid_records_skip_alone() {
        let streams = live_streams(r#"[null, 5, "x", {"name": "News", "stream_id": 7}]"#).unwrap();
        assert_eq!(streams.len(), 1);
        assert!(live_streams("[{").is_err());
    }

    #[test]
    fn stored_line_ups_hold_no_credentials() {
        let streams = live_streams(r#"[{"name": "One", "stream_id": 1}]"#).unwrap();
        let json_api = live_m3u("http://srv", "ts", &[], &streams);
        let export = without_credentials(
            "#EXTM3U\n#EXTINF:-1,One\nhttp://srv/joe/s3cr%3Ft/1\n#EXTINF:-1,Two\nhttp://srv/live/joe/s3cr?t/2.ts\n#EXTINF:-1,Three\nhttp://srv/live.php?username=joe&password=s3cr%3Ft&stream=3\n",
            "joe",
            "s3cr?t",
        );
        for m3u in [&json_api, &export] {
            assert!(!m3u.contains("joe") && !m3u.contains("s3cr"), "{m3u}");
        }
        let played: Vec<String> = parse_m3u_content(&export)
            .entries
            .iter()
            .map(|channel| stream_url(&channel.url, "joe", "s3cr?t"))
            .collect();
        assert_eq!(
            played,
            [
                "http://srv/joe/s3cr%3Ft/1",
                "http://srv/live/joe/s3cr%3Ft/2.ts"
            ]
        );
    }

    #[test]
    fn hostile_panel_fields_stay_on_their_lines() {
        let streams = r#"[
            {"name": "Bad id", "stream_id": "7\nav://lavfi:sine"},
            {"name": "Path id", "stream_id": "../7"},
            {"name": "Float id", "stream_id": 7.5},
            {"name": "N\r\nav://lavfi:sine", "stream_id": 8, "stream_icon": "a\nb",
             "epg_channel_id": "c\rd", "category_id": "1"}
        ]"#;
        let categories = r#"[{"category_id": "1", "category_name": "G\nfdclose://0"}]"#;
        let categories = first_entries(categories, |_: &Category| true).unwrap();
        let streams = live_streams(streams).unwrap();
        let m3u = live_m3u("http://srv\n/x", "ts", &categories, &streams);
        assert_eq!(m3u.lines().count(), 3, "{m3u}");
        let channels = parse_m3u_content(&m3u).entries;
        assert_eq!(channels.len(), 1, "{m3u}");
        assert_eq!(channels[0].url, "http://srv /x/live/{credentials}/8.ts");
    }

    #[test]
    fn streams_past_the_cap_are_skipped() {
        let mut streams = String::from(r#"[{"name": "No id"}"#);
        for id in 0..100_005 {
            streams.push_str(&format!(r#",{{"stream_id": {id}}}"#));
        }
        streams.push(']');
        let streams = live_streams(&streams).unwrap();
        assert_eq!(streams.len(), 100_001);
        assert_eq!(streams[0].stream_id, "0");

        let m3u = live_m3u("http://srv", "ts", &[], &streams);
        let channels = parse_m3u_content(&m3u);
        assert_eq!(
            (channels.entries.len(), channels.truncated),
            (100_000, true)
        );
    }

    #[test]
    fn empty_answers_list_nothing() {
        for answer in ["[]", "{}", "null"] {
            assert!(live_streams(answer).unwrap().is_empty(), "{answer}");
        }
        assert_eq!(live_streams("[").err(), Some(FetchError::Empty));
    }
}
