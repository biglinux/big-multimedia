//! Stateless widget builders for [`RadioPanel`](super::RadioPanel): toolbar,
//! empty-state, loading skeleton, per-row record button + now-playing
//! indicator, async favicon loader, and the add-station dialog. Split out of
//! `radio_panel.rs` (recipe: file-size budget); pure move, behavior identical.
//! All `pub(super)` — called from the `Component` impl and the row builders.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use relm4::ComponentSender;

use big_relm4_components::feedback::tooltip;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};

use crate::media_list::MediaRow;
use big_media_data::db::models::RadioStation;
use big_media_data::i18n::i18n;

use super::{RadioPanel, RadioPanelInput, TAG_EXPLORE, TAG_SAVED};

/// Build the per-row action-button factory for the virtualized list: saved rows
/// get delete + record, explore rows get record + add (add reflects the current
/// saved set). Recreated per visible bind, so it stays cheap.
pub(super) fn row_suffix_builder(
    sender: &ComponentSender<RadioPanel>,
    saved_urls: &Rc<RefCell<HashSet<String>>>,
) -> impl Fn(&MediaRow) -> Vec<gtk::Widget> + 'static {
    let sender = sender.clone();
    let saved_urls = saved_urls.clone();
    move |row| match row.tag {
        TAG_SAVED => {
            let del =
                tooltip::icon_button("edit-delete-symbolic", &i18n("Remove station"), &["flat"]);
            del.set_valign(gtk::Align::Center);
            let s = sender.clone();
            let url = row.id.clone();
            del.connect_clicked(move |_| {
                s.input(RadioPanelInput::DeleteStationByUrl(url.clone()));
            });
            vec![del.upcast(), build_record_button(&sender).upcast()]
        }
        TAG_EXPLORE => {
            let rec = build_record_button(&sender);
            let add =
                tooltip::icon_button("list-add-symbolic", &i18n("Add to my stations"), &["flat"]);
            add.set_valign(gtk::Align::Center);
            if saved_urls.borrow().contains(&row.id) {
                add.set_sensitive(false);
                add.set_icon_name("object-select-symbolic");
            } else {
                let s = sender.clone();
                let station = RadioStation {
                    id: 0,
                    name: row.title.clone(),
                    url: row.id.clone(),
                    favicon: row.favicon.clone(),
                    group_name: row.group.clone(),
                };
                add.connect_clicked(move |button| {
                    s.input(RadioPanelInput::AddStation(station.clone()));
                    button.set_sensitive(false);
                    button.set_icon_name("object-select-symbolic");
                });
            }
            vec![rec.upcast(), add.upcast()]
        }
        _ => Vec::new(),
    }
}

/// Build a record toggle button that flips its own icon and emits RecordToggle.
pub(super) fn build_record_button(sender: &ComponentSender<RadioPanel>) -> gtk::Button {
    let rec_button =
        tooltip::icon_button("media-record-symbolic", &i18n("Record Stream"), &["flat"]);
    rec_button.set_valign(gtk::Align::Center);
    let s = sender.clone();
    rec_button.connect_clicked(move |button| {
        let recording = button
            .icon_name()
            .is_some_and(|n| n == "media-playback-stop-symbolic");
        if recording {
            s.input(RadioPanelInput::RecordToggle(false));
            button.set_icon_name("media-record-symbolic");
            tooltip::update(button, i18n("Record Stream"));
            button.update_property(&[gtk::accessible::Property::Label(&i18n("Record Stream"))]);
        } else {
            s.input(RadioPanelInput::RecordToggle(true));
            button.set_icon_name("media-playback-stop-symbolic");
            tooltip::update(button, i18n("Stop Recording"));
            button.update_property(&[gtk::accessible::Property::Label(&i18n("Stop Recording"))]);
        }
    });
    rec_button
}

/// Build the loading skeleton (6 placeholder rows, hidden initially).
pub(super) fn build_skeleton() -> gtk::Box {
    const SKELETON_ROWS: usize = 6;
    let skeleton = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_start(16)
        .margin_end(16)
        .margin_top(8)
        .visible(false)
        .build();
    for _ in 0..SKELETON_ROWS {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        row.append(
            &gtk::Box::builder()
                .width_request(24)
                .height_request(24)
                .css_classes(["skeleton-circle"])
                .valign(gtk::Align::Center)
                .build(),
        );
        let lines = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        lines.append(
            &gtk::Box::builder()
                .height_request(12)
                .css_classes(["skeleton-line"])
                .build(),
        );
        lines.append(
            &gtk::Box::builder()
                .height_request(8)
                .width_request(120)
                .halign(gtk::Align::Start)
                .css_classes(["skeleton-line"])
                .build(),
        );
        row.append(&lines);
        skeleton.append(&row);
    }
    skeleton
}

/// Build the toolbar (search + genre dropdown + add button).
pub(super) fn build_toolbar() -> (gtk::Box, gtk::SearchEntry, gtk::DropDown, gtk::Button) {
    let toolbar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_start(8)
        .margin_end(8)
        .margin_top(4)
        .margin_bottom(4)
        .build();

    let search_entry = gtk::SearchEntry::builder()
        .hexpand(true)
        .placeholder_text(i18n("Search radios..."))
        .build();
    search_entry.update_property(&[gtk::accessible::Property::Label(&i18n("Search radios"))]);
    toolbar.append(&search_entry);

    let filter_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let genre_items = gtk::StringList::new(&[
        &i18n("All Genres"),
        "pop",
        "rock",
        "news",
        "classical",
        "talk",
        "dance",
        "hits",
        "oldies",
        "jazz",
        "electronic",
        "hip hop",
        "80s",
        "90s",
        "country",
        "top 40",
        "classic rock",
        "blues",
        "house",
        "metal",
        "reggae",
        "latin",
        "folk",
        "ambient",
        "indie",
        "soul",
        "punk",
        "r&b",
        "alternative",
        "christian",
        "reggaeton",
        "world",
    ]);
    let genre_dropdown = gtk::DropDown::builder()
        .model(&genre_items)
        .hexpand(true)
        .css_classes(["flat"])
        .build();
    tooltip::set(&genre_dropdown, i18n("Filter by genre"));
    genre_dropdown.update_property(&[gtk::accessible::Property::Label(&i18n("Filter by genre"))]);
    filter_row.append(&genre_dropdown);

    let add_button = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .css_classes(["flat"])
        .build();
    tooltip::set(&add_button, i18n("Add station manually"));
    add_button.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Add station manually",
    ))]);
    filter_row.append(&add_button);
    toolbar.append(&filter_row);

    (toolbar, search_entry, genre_dropdown, add_button)
}

/// Wire the toolbar widgets to component inputs.
pub(super) fn wire_toolbar(
    search_entry: &gtk::SearchEntry,
    genre_dropdown: &gtk::DropDown,
    add_button: &gtk::Button,
    sender: &ComponentSender<RadioPanel>,
) {
    let s = sender.clone();
    search_entry.connect_search_changed(move |entry| {
        s.input(RadioPanelInput::SearchQueryChanged(
            entry.text().to_string(),
        ));
    });
    let s = sender.clone();
    genre_dropdown.connect_selected_notify(move |dropdown| {
        let idx = dropdown.selected();
        if idx == 0 {
            s.input(RadioPanelInput::GenreSelected(None));
            return;
        }
        let genre = dropdown
            .model()
            .and_then(|model| {
                model
                    .downcast_ref::<gtk::StringList>()
                    .and_then(|list| list.string(idx).map(|v| v.to_string()))
            })
            .unwrap_or_default();
        s.input(RadioPanelInput::GenreSelected(Some(genre)));
    });
    let s = sender.clone();
    add_button.connect_clicked(move |_| s.input(RadioPanelInput::AddDialogRequested));
}

/// Build the zero-stations empty-state panel.
pub(super) fn build_empty_state(sender: &ComponentSender<RadioPanel>) -> gtk::Box {
    let empty_state = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(48)
        .margin_bottom(48)
        .margin_start(24)
        .margin_end(24)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();
    empty_state.append(
        &gtk::Image::builder()
            .icon_name("icon_radio")
            .pixel_size(64)
            .css_classes(["dim-label"])
            .build(),
    );
    empty_state.append(
        &gtk::Label::builder()
            .label(i18n("No radio stations"))
            .css_classes(["title-3"])
            .build(),
    );
    empty_state.append(
        &gtk::Label::builder()
            .label(i18n(
                "Explore popular stations from your country or search by name",
            ))
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build(),
    );
    let explore_button = gtk::Button::builder()
        .label(i18n("Explore Stations"))
        .css_classes(["suggested-action", "pill"])
        .halign(gtk::Align::Center)
        .build();
    let s = sender.clone();
    explore_button.connect_clicked(move |_| s.input(RadioPanelInput::LoadPopular));
    empty_state.append(&explore_button);
    let add_button = gtk::Button::builder()
        .label(i18n("Add Manually"))
        .css_classes(["flat"])
        .halign(gtk::Align::Center)
        .build();
    let s = sender.clone();
    add_button.connect_clicked(move |_| s.input(RadioPanelInput::AddDialogRequested));
    empty_state.append(&add_button);
    empty_state
}

/// Present the add-station dialog parented to the panel's toplevel.
pub(super) fn present_add_dialog(root: &gtk::Box, sender: &ComponentSender<RadioPanel>) {
    let Some(window) = root.root().and_downcast::<gtk::Window>() else {
        return;
    };

    let dialog = adw::Dialog::builder()
        .title(i18n("Add Radio Station"))
        .content_width(440)
        .content_height(300)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);
    page.append_description(&i18n(
        "Enter a station name and an http:// or https:// stream URL.",
    ));

    let group = BigPreferencesGroupSpec::new(i18n("Station")).build();
    let name_entry = adw::EntryRow::builder()
        .title(i18n("Station Name"))
        .show_apply_button(true)
        .build();
    name_entry.update_property(&[gtk::accessible::Property::Label(&i18n("Station Name"))]);
    let url_entry = adw::EntryRow::builder()
        .title(i18n("Station stream URL"))
        .show_apply_button(true)
        .build();
    url_entry.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Station stream URL",
    ))]);
    group.add(&name_entry);
    group.add(&url_entry);
    page.content().append(&group);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Add")));
    footer.accept_button().set_sensitive(false);
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    wire_add_station_validation(&name_entry, &url_entry, footer.accept_button());

    let name_entry_weak = name_entry.downgrade();
    let url_entry_weak = url_entry.downgrade();
    let accept_button_weak = footer.accept_button().downgrade();
    name_entry.connect_apply(move |row| {
        let Some(url_entry) = url_entry_weak.upgrade() else {
            return;
        };
        let Some(accept_button) = accept_button_weak.upgrade() else {
            return;
        };
        if accepted_radio_station_form(&row.text(), &url_entry.text()).is_some() {
            accept_button.emit_clicked();
        }
    });

    let accept_button_weak = footer.accept_button().downgrade();
    url_entry.connect_apply(move |row| {
        let Some(name_entry) = name_entry_weak.upgrade() else {
            return;
        };
        let Some(accept_button) = accept_button_weak.upgrade() else {
            return;
        };
        if accepted_radio_station_form(&name_entry.text(), &row.text()).is_some() {
            accept_button.emit_clicked();
        }
    });

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let s = sender.clone();
    let dialog_weak = dialog.downgrade();
    let name_entry_for_accept = name_entry.clone();
    let url_entry_for_accept = url_entry.clone();
    footer.accept_button().connect_clicked(move |_| {
        let Some(station) = accepted_radio_station_form(
            &name_entry_for_accept.text(),
            &url_entry_for_accept.text(),
        ) else {
            return;
        };
        s.input(RadioPanelInput::AddStation(RadioStation {
            id: 0,
            name: station.name,
            url: station.url,
            favicon: String::new(),
            group_name: String::new(),
        }));
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    dialog.connect_map(move |_| {
        let name_entry = name_entry.downgrade();
        gtk::glib::idle_add_local_once(move || {
            if let Some(name_entry) = name_entry.upgrade() {
                name_entry.grab_focus();
            }
        });
    });
    dialog.present(Some(&window));
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RadioStationForm {
    name: String,
    url: String,
}

fn accepted_radio_station_form(name: &str, url: &str) -> Option<RadioStationForm> {
    let name = name.trim();
    let url = url.trim();
    if name.is_empty() {
        return None;
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }
    let (_, target) = url.split_once("://")?;
    if target.trim().is_empty() {
        return None;
    }
    Some(RadioStationForm {
        name: name.to_owned(),
        url: url.to_owned(),
    })
}

fn wire_add_station_validation(
    name_entry: &adw::EntryRow,
    url_entry: &adw::EntryRow,
    accept_button: &gtk::Button,
) {
    let url_entry_weak = url_entry.downgrade();
    let accept_button_weak = accept_button.downgrade();
    name_entry.connect_changed(move |row| {
        let Some(url_entry) = url_entry_weak.upgrade() else {
            return;
        };
        let Some(accept_button) = accept_button_weak.upgrade() else {
            return;
        };
        sync_add_station_accept_button(row, &url_entry, &accept_button);
    });

    let name_entry_weak = name_entry.downgrade();
    let accept_button_weak = accept_button.downgrade();
    url_entry.connect_changed(move |row| {
        let Some(name_entry) = name_entry_weak.upgrade() else {
            return;
        };
        let Some(accept_button) = accept_button_weak.upgrade() else {
            return;
        };
        sync_add_station_accept_button(&name_entry, row, &accept_button);
    });
}

fn sync_add_station_accept_button(
    name_entry: &adw::EntryRow,
    url_entry: &adw::EntryRow,
    accept_button: &gtk::Button,
) {
    accept_button.set_sensitive(
        accepted_radio_station_form(&name_entry.text(), &url_entry.text()).is_some(),
    );
}

#[cfg(test)]
mod tests {
    use super::{RadioStationForm, accepted_radio_station_form};

    #[test]
    fn accepted_radio_station_form_trims_valid_http_inputs() {
        assert_eq!(
            accepted_radio_station_form(
                "  Codex Radio  ",
                "  https://stream.example.test/live.mp3  "
            ),
            Some(RadioStationForm {
                name: "Codex Radio".into(),
                url: "https://stream.example.test/live.mp3".into(),
            })
        );
    }

    #[test]
    fn accepted_radio_station_form_rejects_empty_or_non_http_inputs() {
        assert_eq!(
            accepted_radio_station_form("", "https://example.test/live"),
            None
        );
        assert_eq!(accepted_radio_station_form("Radio", ""), None);
        assert_eq!(
            accepted_radio_station_form("Radio", "file:///tmp/radio.mp3"),
            None
        );
        assert_eq!(accepted_radio_station_form("Radio", "https://   "), None);
    }
}
