use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::super::{BrowserState, FilterKind, schedule_search};
use crate::i18n::{i18n, ni18n};
use crate::media::playlist;

/// Kind chips: All / Folders / Videos (session-local; not persisted).
pub(crate) fn build_chips(state: &Rc<BrowserState>) {
    let chips = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["linked"])
        .halign(gtk::Align::Center)
        .margin_start(8)
        .margin_end(8)
        .margin_bottom(4)
        .build();

    let active = state.filter_kind.get();
    let mut group: Option<gtk::ToggleButton> = None;
    for (kind, label) in [
        (FilterKind::All, i18n("All")),
        (FilterKind::Folders, i18n("Folders")),
        (FilterKind::Videos, i18n("Videos")),
    ] {
        let filter_chip_button = gtk::ToggleButton::builder()
            .label(&label)
            .active(kind == active)
            .css_classes(["flat"])
            .build();
        if let Some(first) = &group {
            filter_chip_button.set_group(Some(first));
        } else {
            group = Some(filter_chip_button.clone());
        }
        filter_chip_button.connect_toggled({
            let state = state.clone();
            move |button| {
                if button.is_active() {
                    state.filter_kind.set(kind);
                    state.refilter();
                }
            }
        });
        chips.append(&filter_chip_button);
    }
    if let Some(panel) = state.container() {
        panel.append(&chips);
    }
}

/// Dolphin-style breadcrumb, led by "Your folders", the way back to the
/// library's places.
pub(crate) fn build_breadcrumb(state: &Rc<BrowserState>, folder: &str) {
    let segments = playlist::breadcrumb_segments(folder);

    let bar = gtk::Box::builder().spacing(0).build();
    bar.add_css_class("breadcrumb-bar");

    let places = gtk::Button::builder()
        .icon_name("user-bookmarks-symbolic")
        .tooltip_text(i18n("Your folders"))
        .css_classes(["flat", "breadcrumb-btn"])
        .build();
    places.update_property(&[gtk::accessible::Property::Label(&i18n("Your folders"))]);
    {
        let state = state.clone();
        places.connect_clicked(move |_| state.show_places());
    }
    bar.append(&places);

    let last = segments.len() - 1;
    for (i, (name, path)) in segments.iter().enumerate() {
        let sep = gtk::Label::builder().label("›").build();
        sep.add_css_class("dim-label");
        sep.add_css_class("breadcrumb-sep");
        bar.append(&sep);
        let breadcrumb_button = gtk::Button::builder().label(name).build();
        breadcrumb_button.add_css_class("flat");
        breadcrumb_button.add_css_class("breadcrumb-btn");
        if i == last {
            breadcrumb_button.set_sensitive(false);
            breadcrumb_button.add_css_class("accent");
        } else {
            let path = path.clone();
            let state = state.clone();
            breadcrumb_button.connect_clicked(move |_| state.navigate_to_folder(&path));
        }
        bar.append(&breadcrumb_button);
    }

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .margin_start(8)
        .margin_end(8)
        .margin_bottom(4)
        .child(&bar)
        .build();
    // Scroll to the deepest crumb so the active folder is visible.
    let scroll_weak = scroll.downgrade();
    glib::idle_add_local_once(move || {
        if let Some(sw) = scroll_weak.upgrade() {
            let adj = sw.hadjustment();
            adj.set_value(adj.upper());
        }
    });
    if let Some(panel) = state.container() {
        panel.append(&scroll);
    }
}

/// Debounced fuzzy search entry.
pub(crate) fn build_search(state: &Rc<BrowserState>) -> gtk::SearchEntry {
    let entry = gtk::SearchEntry::builder()
        .placeholder_text(i18n("Filter…"))
        .margin_start(8)
        .margin_end(8)
        .margin_bottom(4)
        .build();
    entry.connect_search_changed({
        let state = state.clone();
        move |e| schedule_search(&state, e.text().to_string().to_lowercase())
    });
    if let Some(panel) = state.container() {
        panel.append(&entry);
    }
    entry
}

/// Info row: just the video count (+ a truncation warning when a recursive scan
/// hit its cap). The folder path is *not* repeated here — the breadcrumb above
/// already shows it.
pub(crate) fn build_count_summary(state: &Rc<BrowserState>, count: usize) {
    let row = gtk::Box::builder()
        .spacing(6)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(4)
        .build();

    let count_text = ni18n("{} video", "{} videos", count as u64).replace("{}", &count.to_string());
    let count_label = caption_label(&count_text);
    count_label.set_halign(gtk::Align::Start);
    row.append(&count_label);

    if let Some(total) = state.truncated_total.get() {
        let warn = gtk::Label::builder()
            .label(i18n("(of {total})").replace("{total}", &total.to_string()))
            .css_classes(["warning", "caption"])
            .build();
        warn.set_tooltip_text(Some(
            &i18n("Showing {shown} of {total} files found. Consider filtering by subfolder.")
                .replace("{shown}", &count.to_string())
                .replace("{total}", &total.to_string()),
        ));
        row.append(&warn);
    }

    if let Some(panel) = state.container() {
        panel.append(&row);
    }
}

fn caption_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .css_classes(["dim-label", "caption"])
        .build()
}
