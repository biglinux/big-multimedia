use std::rc::Rc;

use adw::prelude::*;

use super::super::{BrowserState, SidebarMode, VideoBrowserSidebarOutput};
use super::header::{add_files, pick_folder};
use crate::i18n::i18n;
use crate::settings::keys;

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

/// Playlist mode empty state: the queue has nothing in it. Add files, go
/// to a folder in the other tab, or drop videos on the panel.
pub(crate) fn build_playlist_empty_state(state: &Rc<BrowserState>) {
    let add = pill_button(&i18n("Add Files"), &["pill", "suggested-action"], {
        let state = state.clone();
        move || add_files(&state)
    });
    let folder = pill_button(&i18n("Choose from a folder"), &["pill"], {
        let state = state.clone();
        move || {
            state
                .sender
                .output(VideoBrowserSidebarOutput::ShowTab(SidebarMode::Files))
                .ok();
        }
    });
    build_status_page(
        state,
        "view-list-symbolic",
        &i18n("Your playlist is empty"),
        &i18n("Drop videos here, or pick them in a folder"),
        &[add, folder],
    );
}

/// Shown once the playlist has two videos, until closed: how to reorder.
pub(crate) fn build_reorder_hint(state: &Rc<BrowserState>) {
    let row = gtk::Box::builder()
        .spacing(8)
        .margin_start(8)
        .margin_end(8)
        .margin_bottom(4)
        .css_classes(["panel-backed", "playlist-hint"])
        .build();
    row.append(&gtk::Image::from_icon_name("list-drag-handle-symbolic"));
    row.append(
        &gtk::Label::builder()
            .label(i18n(
                "Drag a video by its handle to change the order, or use Alt+Up and Alt+Down",
            ))
            .wrap(true)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["caption"])
            .build(),
    );
    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .tooltip_text(i18n("Got it"))
        .build();
    close.update_property(&[gtk::accessible::Property::Label(&i18n("Got it"))]);
    {
        let state = state.clone();
        let row = row.downgrade();
        close.connect_clicked(move |_| {
            let _ = state
                .settings
                .put_bool(keys::PLAYLIST_REORDER_HINT_SEEN, true);
            if let Some(row) = row.upgrade() {
                row.set_visible(false);
            }
        });
    }
    row.append(&close);
    if let Some(panel) = state.container() {
        panel.append(&row);
    }
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

    // Stacked: side by side they did not fit the panel's width.
    let button_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
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
