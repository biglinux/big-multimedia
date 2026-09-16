use gtk::glib::{self, Variant};
use glib::variant::{ObjectPath, ToVariant};
use super::types::{PlayerState, micros};
pub(super) fn build_metadata(state: &PlayerState) -> Variant {
    let dict = glib::VariantDict::new(None);
    let Some(id) = state.track_id.as_deref().and_then(|id| ObjectPath::try_from(id).ok()) else { return dict.end(); };
    dict.insert_value("mpris:trackid", &id.to_variant());
    dict.insert("xesam:title", &state.title);
    if !state.artist.is_empty() { dict.insert("xesam:artist", &vec![state.artist.clone()]); }
    if !state.album.is_empty() { dict.insert("xesam:album", &state.album); }
    let length = micros(state.duration_secs);
    if length > 0 { dict.insert("mpris:length", length); }
    if !state.art_url.is_empty() { dict.insert("mpris:artUrl", &state.art_url); }
    dict.end()
}
