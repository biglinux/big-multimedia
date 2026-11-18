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
//! same as for an M3U provider. Engine-free helper; blocking, so call it from
//! a worker thread.

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

/// The live line-up of a Xtream account as M3U text.
pub fn fetch_xtream_m3u(
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
        return fetch_text(&get_php_url(base, username, password), headers);
    };
    let extension = check_account(&account)?;
    let categories = fetch_text(&format!("{api}&action=get_live_categories"), headers)?;
    let streams = fetch_text(&format!("{api}&action=get_live_streams"), headers)?;
    let categories = first_entries(&categories, |_: &Category| true)?;
    let streams = live_streams(&streams)?;
    let m3u = live_m3u(base, username, password, extension, &categories, &streams);
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

/// The streams of a `get_live_streams` answer that have an id to play.
fn live_streams(json: &str) -> Result<Vec<LiveStream>, FetchError> {
    first_entries(json, |stream: &LiveStream| !stream.stream_id.is_empty())
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

impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for FirstEntries<T> {
    type Value = Vec<T>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<T>, D::Error> {
        deserializer.deserialize_any(self)
    }
}

/// A panel with nothing to list may answer an object or `null`.
impl<'de, T: Deserialize<'de>> Visitor<'de> for FirstEntries<T> {
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
            let Some(entry) = seq.next_element::<T>()? else {
                return Ok(entries);
            };
            if (self.keep)(&entry) {
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
pub fn get_php_url(base: &str, username: &str, password: &str) -> String {
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

/// An M3U attribute value: a double quote would end it early.
fn attribute(text: &str) -> String {
    text.replace('"', "'").replace(['\n', '\r'], " ")
}

/// The live streams as M3U, grouped by their category names.
fn live_m3u(
    base: &str,
    username: &str,
    password: &str,
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
    let mut m3u = String::from("#EXTM3U\n");
    for stream in streams {
        let group = names
            .get(stream.category_id.as_str())
            .copied()
            .unwrap_or_default();
        m3u.push_str(&format!(
            "#EXTINF:-1 tvg-id=\"{}\" tvg-logo=\"{}\" group-title=\"{}\",{}\n{base}/live/{}/{}/{}.{extension}\n",
            attribute(&stream.epg_channel_id),
            attribute(&stream.stream_icon),
            attribute(group),
            stream.name.replace(['\n', '\r'], " "),
            urlencode(username),
            urlencode(password),
            urlencode(&stream.stream_id),
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
    fn urlencodes_special_chars() {
        assert_eq!(urlencode("user@example"), "user%40example");
        assert_eq!(urlencode("pa&ss"), "pa%26ss");
        assert_eq!(urlencode("plain123-_.~"), "plain123-_.~");
    }

    #[test]
    fn urlencodes_plus_and_equal() {
        assert_eq!(urlencode("a+b=c"), "a%2Bb%3Dc");
    }

    #[test]
    fn urlencodes_empty_string() {
        assert_eq!(urlencode(""), "");
    }

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
        let m3u = live_m3u("http://srv", "u@x", "p", "m3u8", &categories, &streams);
        assert!(m3u.contains("tvg-id=\"globo.br\""));
        let channels = parse_m3u_content(&m3u).entries;
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Globo, SP");
        assert_eq!(channels[0].group, "News");
        assert_eq!(channels[0].logo, "http://logo/1.png");
        assert_eq!(channels[0].url, "http://srv/live/u%40x/p/101.m3u8");
        assert_eq!(channels[1].group, "Sports 'HD'");
        assert_eq!(channels[1].url, "http://srv/live/u%40x/p/202.m3u8");
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

        let m3u = live_m3u("http://srv", "u", "p", "ts", &[], &streams);
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
