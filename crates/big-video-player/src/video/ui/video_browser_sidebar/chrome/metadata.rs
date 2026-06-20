use std::rc::Rc;

use adw::prelude::*;
use big_relm4_components::feedback::tooltip;
use gtk::glib;

use super::super::{schedule_search, BrowserState, FilterKind};
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
        let btn = gtk::ToggleButton::builder()
            .label(&label)
            .active(kind == active)
            .css_classes(["flat"])
            .build();
        if let Some(first) = &group {
            btn.set_group(Some(first));
        } else {
            group = Some(btn.clone());
        }
        btn.connect_toggled({
            let state = state.clone();
            move |b| {
                if b.is_active() {
                    state.filter_kind.set(kind);
                    state.refilter();
                }
            }
        });
        chips.append(&btn);
    }
    if let Some(panel) = state.container() {
        panel.append(&chips);
    }
}

/// Dolphin-style breadcrumb; hidden for root paths (single segment).
pub(crate) fn build_breadcrumb(state: &Rc<BrowserState>, folder: &str) {
    let segments = playlist::breadcrumb_segments(folder);
    if segments.len() <= 1 {
        return;
    }

    let bar = gtk::Box::builder().spacing(0).build();
    bar.add_css_class("breadcrumb-bar");

    let home = gtk::Image::from_icon_name("user-home-symbolic");
    home.set_pixel_size(14);
    home.set_margin_start(6);
    home.set_margin_end(2);
    bar.append(&home);

    let last = segments.len() - 1;
    for (i, (name, path)) in segments.iter().enumerate() {
        if i > 0 {
            let sep = gtk::Label::builder().label("›").build();
            sep.add_css_class("dim-label");
            sep.add_css_class("breadcrumb-sep");
            bar.append(&sep);
        }
        let btn = gtk::Button::builder().label(name).build();
        btn.add_css_class("flat");
        btn.add_css_class("breadcrumb-btn");
        if i == last {
            btn.set_sensitive(false);
            btn.add_css_class("accent");
        } else {
            let path = path.clone();
            let state = state.clone();
            btn.connect_clicked(move |_| state.navigate_to_folder(&path));
        }
        bar.append(&btn);
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
pub(crate) fn build_info(state: &Rc<BrowserState>, count: usize) {
    let row = gtk::Box::builder()
        .spacing(6)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(4)
        .build();

    let count_text = ni18n("{} video", "{} videos", count as u64).replace("{}", &count.to_string());
    let count_lbl = caption_label(&count_text);
    count_lbl.set_halign(gtk::Align::Start);
    row.append(&count_lbl);

    if state.truncated.get() {
        let total = state.total_found.get();
        let warn = gtk::Label::builder()
            .label(i18n("(of {total})").replace("{total}", &total.to_string()))
            .css_classes(["warning", "caption"])
            .build();
        tooltip::set(
            &warn,
            &i18n("Showing {shown} of {total} files found. Consider filtering by subfolder.")
                .replace("{shown}", &count.to_string())
                .replace("{total}", &total.to_string()),
        );
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
