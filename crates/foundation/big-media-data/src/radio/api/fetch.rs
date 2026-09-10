//! Radio Browser API fetchers: search by name, by country, by tag, top-click.
//! Free functions — call OFF the main thread (they block on HTTP). Resolve the
//! server list with [`api_servers_from_raw`](super::api_servers_from_raw) from a
//! raw settings string read on the main thread via
//! [`settings_servers_raw`](super::settings_servers_raw).

use big_os_kit::url::encode_query_value as urlencoding_encode;

use super::super::RadioExploreEntry;
use super::discovery::radio_browser_get_with_fallback;
use super::parse::{parse_stations_json, parse_stations_json_filtered};

pub fn search_radio_browser_api(query: &str, servers: &[String]) -> Option<Vec<RadioExploreEntry>> {
    let path = format!(
        "/json/stations/search?name={}&limit=250&order=clickcount&reverse=true&hidebroken=true",
        urlencoding_encode(query)
    );
    let body = radio_browser_get_with_fallback(&path, servers)?;
    Some(parse_stations_json(&body))
}

pub fn fetch_stations_by_country(
    country_code: &str,
    limit: u32,
    servers: &[String],
) -> Option<Vec<RadioExploreEntry>> {
    let path = format!(
        "/json/stations/bycountrycodeexact/{}?order=clickcount&reverse=true&limit={limit}&hidebroken=true",
        urlencoding_encode(country_code)
    );
    let body = radio_browser_get_with_fallback(&path, servers)?;
    Some(parse_stations_json_filtered(&body))
}

pub fn fetch_stations_by_tag(
    tag: &str,
    limit: u32,
    servers: &[String],
) -> Option<Vec<RadioExploreEntry>> {
    let path = format!(
        "/json/stations/bytag/{}?order=clickcount&reverse=true&limit={limit}&hidebroken=true",
        urlencoding_encode(tag)
    );
    let body = radio_browser_get_with_fallback(&path, servers)?;
    Some(parse_stations_json_filtered(&body))
}

pub fn fetch_top_stations(limit: u32, servers: &[String]) -> Option<Vec<RadioExploreEntry>> {
    let path = format!("/json/stations/topclick/{limit}?hidebroken=true");
    let body = radio_browser_get_with_fallback(&path, servers)?;
    Some(parse_stations_json_filtered(&body))
}
