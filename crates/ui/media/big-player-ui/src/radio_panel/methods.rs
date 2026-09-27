//! [`RadioPanel`](super::RadioPanel) behavior: the generation-guarded async
//! query, loading state, and the combined [`MediaRow`] list construction (My
//! Stations + Explore sections). Rendering is a model swap into the virtualized
//! [`MediaListView`](crate::media_list) — no per-row widget build, no favicon
//! fetch storm.

use adw::prelude::*;
use gtk::{gio, glib};
use relm4::ComponentSender;

use crate::favicon_cache::fetch_favicon_bytes;
use crate::media_list::{MediaRow, RowImage};
use big_media_data::db::models::RadioStation;
use big_media_data::i18n::i18n;
use big_media_data::radio::api::{
    api_servers_from_raw, detect_country_code, fetch_stations_by_country, fetch_stations_by_tag,
    fetch_top_stations, search_radio_browser_api, settings_servers_raw,
};
use big_media_data::settings::Settings;

use super::{RadioPanel, RadioPanelInput, TAG_EXPLORE, TAG_SAVED};

/// Leading-image pixel size for a station row.
const ICON_PX: i32 = 24;

/// Internal kinds for the generation-counter-guarded async query.
pub(super) enum QueryKind {
    Name(String),
    Tag(String),
    Popular,
}

impl RadioPanel {
    /// Resolve a station's (name, favicon) from explore results or saved list.
    pub(super) fn resolve_station_meta(&self, url: &str) -> (String, String) {
        if let Some((name, _, _, _, favicon)) = self.explore_data.iter().find(|e| e.1 == url) {
            return (name.clone(), favicon.clone());
        }
        if let Some(station) = self.stations.iter().find(|s| s.url == url) {
            return (station.name.clone(), station.favicon.clone());
        }
        (String::new(), String::new())
    }

    /// Bump the generation counter and spawn the off-thread query.
    pub(super) fn start_query(&mut self, kind: QueryKind, sender: &ComponentSender<Self>) {
        let has_cached = !self.explore_data.is_empty();
        let popular = matches!(kind, QueryKind::Popular);
        // Name search always shows loading; popular/genre skip the spinner when
        // cached results are already on screen.
        if matches!(kind, QueryKind::Name(_)) || !has_cached {
            self.show_loading();
        }
        self.search_gen = self.search_gen.wrapping_add(1);
        let generation = self.search_gen;

        let servers_raw = settings_servers_raw(&Settings::for_audio());
        let s = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let results = gio::spawn_blocking(move || {
                let servers = api_servers_from_raw(&servers_raw);
                match kind {
                    QueryKind::Name(q) => search_radio_browser_api(&q, &servers),
                    QueryKind::Tag(tag) => fetch_stations_by_tag(&tag, 100, &servers),
                    QueryKind::Popular => {
                        let cc = detect_country_code();
                        if cc.len() == 2 {
                            fetch_stations_by_country(&cc, 100, &servers)
                        } else {
                            fetch_top_stations(100, &servers)
                        }
                    }
                }
            })
            .await
            .unwrap_or(None);
            let _ = s.send(RadioPanelInput::SearchFinished {
                generation,
                popular,
                results,
            });
        });
    }

    pub(super) fn show_loading(&self) {
        self.spinner.set_visible(true);
        self.skeleton.set_visible(true);
        self.empty_state.set_visible(false);
    }

    pub(super) fn hide_loading(&self) {
        self.spinner.set_visible(false);
        self.skeleton.set_visible(false);
    }

    /// Network failure: keep any cached content and let the
    /// empty-state (with its Explore/retry button) show when there is nothing.
    pub(super) fn show_connection_error(&self) {
        self.hide_loading();
        self.rebuild_list();
    }

    /// Build the combined section list: My Stations (filtered + grouped) then
    /// Explore, as flat rows with inline section headers.
    pub(super) fn build_rows(&self) -> Vec<MediaRow> {
        let mut rows = Vec::new();

        let filter_lower = self.filter.to_lowercase();
        let filtered: Vec<&RadioStation> = if self.filter.is_empty() {
            self.stations.iter().collect()
        } else {
            self.stations
                .iter()
                .filter(|s| {
                    s.name.to_lowercase().contains(&filter_lower)
                        || s.group_name.to_lowercase().contains(&filter_lower)
                })
                .collect()
        };

        if !filtered.is_empty() {
            rows.push(MediaRow::header(i18n("My Stations")));
            let mut groups: std::collections::BTreeMap<String, Vec<&RadioStation>> =
                std::collections::BTreeMap::new();
            for station in &filtered {
                let group = if station.group_name.is_empty() {
                    i18n("Ungrouped")
                } else {
                    station.group_name.clone()
                };
                groups.entry(group).or_default().push(station);
            }
            let single_group = groups.len() <= 1;
            for (group_name, group_stations) in &groups {
                if !single_group {
                    rows.push(MediaRow::header(group_name.clone()));
                }
                for station in group_stations {
                    rows.push(station_row(station));
                }
            }
        }

        if !self.explore_data.is_empty() {
            rows.push(MediaRow::header(i18n("Explore Stations")));
            for (name, url, country, tags, favicon) in &self.explore_data {
                rows.push(explore_row(name, url, country, tags, favicon));
            }
        }

        rows
    }
}

/// Build a saved-station [`MediaRow`].
fn station_row(station: &RadioStation) -> MediaRow {
    MediaRow {
        id: station.url.clone(),
        title: station.name.clone(),
        subtitle: None,
        image: station_image(&station.favicon, &station.name),
        header: None,
        favicon: station.favicon.clone(),
        group: station.group_name.clone(),
        tag: TAG_SAVED,
    }
}

/// Build an explore-result [`MediaRow`].
fn explore_row(name: &str, url: &str, country: &str, tags: &str, favicon: &str) -> MediaRow {
    let subtitle = if !country.is_empty() && !tags.is_empty() {
        format!("{country} · {tags}")
    } else if country.is_empty() {
        tags.to_string()
    } else {
        country.to_string()
    };
    MediaRow {
        id: url.to_string(),
        title: name.to_string(),
        subtitle: (!subtitle.is_empty()).then_some(subtitle),
        image: station_image(favicon, name),
        header: None,
        favicon: favicon.to_string(),
        group: String::new(),
        tag: TAG_EXPLORE,
    }
}

/// A favicon image (remote, with initials fallback) or a plain initials avatar.
fn station_image(favicon: &str, name: &str) -> RowImage {
    if favicon.is_empty() {
        RowImage::Initials {
            text: name.to_owned(),
            size: ICON_PX,
        }
    } else {
        RowImage::Remote {
            url: favicon.to_owned(),
            initials: name.to_owned(),
            size: ICON_PX,
            fetch: std::sync::Arc::new(fetch_favicon_bytes),
        }
    }
}
