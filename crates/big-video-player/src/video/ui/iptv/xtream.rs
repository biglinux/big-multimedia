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

use big_os_kit::http_client::RequestHeaders;
use big_os_kit::url::encode_query_value as urlencode;
use serde_json::Value;

use super::fetcher::{FetchError, fetch_text};

/// The live line-up of a Xtream account as M3U text.
pub fn fetch_xtream_m3u(
    server: &str,
    username: &str,
    password: &str,
    headers: &RequestHeaders<'_>,
) -> Result<String, FetchError> {
    let base = server.trim_end_matches('/');
    let api = player_api_url(base, username, password);
    let account = match fetch_text(&api, headers).map(|text| serde_json::from_str::<Value>(&text)) {
        Ok(Ok(account)) => account,
        // No JSON API here: the panel's M3U export.
        Ok(Err(_)) | Err(FetchError::Status(404)) => {
            return fetch_text(&get_php_url(base, username, password), headers);
        }
        Err(error) => return Err(error),
    };
    let extension = check_account(&account)?;
    let categories = fetch_text(&format!("{api}&action=get_live_categories"), headers)?;
    let streams = fetch_text(&format!("{api}&action=get_live_streams"), headers)?;
    let categories: Value = serde_json::from_str(&categories).map_err(|_| FetchError::Empty)?;
    let streams: Value = serde_json::from_str(&streams).map_err(|_| FetchError::Empty)?;
    let m3u = live_m3u(base, username, password, extension, &categories, &streams);
    if m3u.lines().count() <= 1 {
        return Err(FetchError::Empty);
    }
    Ok(m3u)
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

/// Text of a JSON field the panel may send as a string or a number.
fn field(value: &Value, key: &str) -> String {
    match &value[key] {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
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
    categories: &Value,
    streams: &Value,
) -> String {
    let names: std::collections::HashMap<String, String> = categories
        .as_array()
        .into_iter()
        .flatten()
        .map(|category| {
            (
                field(category, "category_id"),
                field(category, "category_name"),
            )
        })
        .collect();
    let mut m3u = String::from("#EXTM3U\n");
    for stream in streams.as_array().into_iter().flatten() {
        let id = field(stream, "stream_id");
        if id.is_empty() {
            continue;
        }
        let group = names
            .get(&field(stream, "category_id"))
            .cloned()
            .unwrap_or_default();
        m3u.push_str(&format!(
            "#EXTINF:-1 tvg-id=\"{}\" tvg-logo=\"{}\" group-title=\"{}\",{}\n{base}/live/{}/{}/{id}.{extension}\n",
            attribute(&field(stream, "epg_channel_id")),
            attribute(&field(stream, "stream_icon")),
            attribute(&group),
            field(stream, "name").replace(['\n', '\r'], " "),
            urlencode(username),
            urlencode(password),
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
        let categories = json!([
            {"category_id": "1", "category_name": "News"},
            {"category_id": 2, "category_name": "Sports \"HD\""}
        ]);
        let streams = json!([
            {"name": "Globo, SP", "stream_id": 101, "stream_icon": "http://logo/1.png",
             "epg_channel_id": "globo.br", "category_id": "1"},
            {"name": "ESPN", "stream_id": "202", "stream_icon": null, "category_id": 2},
            {"name": "No id"}
        ]);
        let m3u = live_m3u("http://srv", "u@x", "p", "m3u8", &categories, &streams);
        let channels = parse_m3u_content(&m3u);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Globo, SP");
        assert_eq!(channels[0].group, "News");
        assert_eq!(channels[0].logo, "http://logo/1.png");
        assert_eq!(channels[0].tvg_id, "globo.br");
        assert_eq!(channels[0].url, "http://srv/live/u%40x/p/101.m3u8");
        assert_eq!(channels[1].group, "Sports 'HD'");
        assert_eq!(channels[1].url, "http://srv/live/u%40x/p/202.m3u8");
    }
}
