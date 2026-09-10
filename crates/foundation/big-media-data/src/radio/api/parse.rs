//! Compatibility adapter for shared Radio Browser parsing.

use super::super::RadioExploreEntry;

pub fn parse_stations_json(body: &str) -> Vec<RadioExploreEntry> {
    big_media_engines::radio_browser::parse_stations_json(body)
}

pub fn parse_stations_json_filtered(body: &str) -> Vec<RadioExploreEntry> {
    big_media_engines::radio_browser::parse_stations_json_filtered(body)
}

pub fn detect_country_code() -> String {
    big_media_engines::radio_browser::detect_country_code()
}
