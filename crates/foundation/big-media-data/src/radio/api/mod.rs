//! Radio Browser API client: server discovery and fetchers.

mod discovery;
mod fetch;

pub use big_media_engines::radio_browser::detect_country_code;
pub use discovery::{api_servers_from_raw, settings_servers_raw};
pub use fetch::{
    fetch_stations_by_country, fetch_stations_by_tag, fetch_top_stations, search_radio_browser_api,
};
