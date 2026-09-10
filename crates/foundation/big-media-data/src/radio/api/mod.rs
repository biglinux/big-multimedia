//! Radio Browser API client: server discovery, fetchers, JSON parsers.

mod discovery;
mod fetch;
mod parse;

pub use discovery::{api_servers_from_raw, settings_servers_raw};
pub use fetch::{
    fetch_stations_by_country, fetch_stations_by_tag, fetch_top_stations, search_radio_browser_api,
};
pub use parse::detect_country_code;
