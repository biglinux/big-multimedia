use std::rc::Rc;

use adw::prelude::*;

use super::super::BrowserState;
use super::header::{add_files, pick_folder};
use crate::i18n::i18n;

/// Files mode empty state: nothing browsed yet → load a folder / add files.
pub(crate) fn build_files_empty_state(state: &Rc<BrowserState>) {
    let folder = pill_button(&i18n("Load Folder"), &["pill", "suggested-action"], {
        let state = state.clone();
        move || pick_folder(&state)
    });
    let add = pill_button(&i18n("Add Files"), &["pill"], {
        let state = state.clone();
        move || add_files(&state)
    });
    build_status_page(
        state,
        "folder-open-symbolic",
        &i18n("Browse your videos"),
        &i18n("Load a folder to explore, or add files to the playlist"),
        &[folder, add],
    );
}

/// Playlist mode empty state: the queue has nothing in it. Only "Add files" —
/// browsing the disk is the separate Files tab's job, so no Browse button here.
pub(crate) fn build_playlist_empty_state(state: &Rc<BrowserState>) {
    let add = pill_button(&i18n("Add Files"), &["pill", "suggested-action"], {
        let state = state.clone();
        move || add_files(&state)
    });
    build_status_page(
        state,
        "view-list-symbolic",
        &i18n("Your playlist is empty"),
        &i18n("Add files to start watching"),
        &[add],
    );
}

/// Centered status page with an icon, title, description, and a row of pills.
fn build_status_page(
    state: &Rc<BrowserState>,
    icon: &str,
    title: &str,
    description: &str,
    buttons: &[gtk::Button],
) {
    let status = adw::StatusPage::builder()
        .icon_name(icon)
        .title(title)
        .description(description)
        .css_classes(["compact"])
        .build();

    let button_row = gtk::Box::builder()
        .spacing(12)
        .halign(gtk::Align::Center)
        .margin_top(8)
        .build();
    for btn in buttons {
        button_row.append(btn);
    }

    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .valign(gtk::Align::Center)
        .build();
    container.append(&status);
    container.append(&button_row);
    if let Some(panel) = state.container() {
        panel.append(&container);
    }
}

/// A pill button wired to `on_click`.
fn pill_button(label: &str, classes: &[&str], on_click: impl Fn() + 'static) -> gtk::Button {
    let pill_button = gtk::Button::builder()
        .label(label)
        .css_classes(classes)
        .build();
    pill_button.connect_clicked(move |_| on_click());
    pill_button
}
