use std::rc::Rc;

use adw::prelude::*;

use super::super::{BrowserState, SidebarMode, VideoBrowserSidebarOutput, fileops};
use crate::i18n::i18n;

/// Per-row right-click menu (Play / Remove / Open containing folder). Keyed by
/// `path` (not a row index) so a position shift between bind and click can never
/// target the wrong entry.
pub(crate) fn show_row_context_menu(
    state: &Rc<BrowserState>,
    anchor: &gtk::Widget,
    path: &str,
    x: f64,
    y: f64,
) {
    let menu = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    let popover = gtk::Popover::builder()
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .build();
    popover.set_parent(anchor);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));

    let add_context_menu_button = |label: &str, icon: &str| -> gtk::Button {
        let row = gtk::Box::builder().spacing(8).build();
        row.append(&gtk::Image::from_icon_name(icon));
        row.append(&gtk::Label::builder().label(label).xalign(0.0).build());
        let menu_button = gtk::Button::builder()
            .child(&row)
            .css_classes(["flat"])
            .build();
        menu.append(&menu_button);
        menu_button
    };

    let play = add_context_menu_button(&i18n("Play"), "media-playback-start-symbolic");
    play.connect_clicked({
        let state = state.clone();
        let path = path.to_string();
        // WEAK: menu items live inside the popover, so a strong clone is a
        // popover ⇄ item ⇄ closure cycle that leaks the whole context menu on
        // every right-click (the popover unparents on close but the cycle pins
        // it). Upgrade transiently — it is alive while an item is clickable.
        let popover = popover.downgrade();
        move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            let paths = state.active_items();
            let Some(start) = paths.iter().position(|p| *p == path) else {
                return;
            };
            state.current_path.replace(Some(path.clone()));
            state.recompute_pos();
            state.update_highlight();
            state
                .sender
                .output(VideoBrowserSidebarOutput::PlayFromList { paths, start })
                .ok();
        }
    });

    // Right after the playing one, in either tab: the list is kept.
    let next = add_context_menu_button(&i18n("Play next"), "media-skip-forward-symbolic");
    next.connect_clicked(emit_paths(state, &popover, path, |s, p| {
        s.sender
            .output(VideoBrowserSidebarOutput::PlayNext {
                path: p,
                now: false,
            })
            .ok();
    }));

    // Files mode → everyday per-item actions (bulk org lives in the batch bar);
    // Playlist mode → just Remove.
    if state.mode.get() == SidebarMode::Files {
        let add = add_context_menu_button(&i18n("Add to playlist"), "list-add-symbolic");
        add.connect_clicked(emit_paths(state, &popover, path, |s, p| {
            s.sender
                .output(VideoBrowserSidebarOutput::Enqueue(vec![p]))
                .ok();
        }));

        let rename = add_context_menu_button(&i18n("Rename…"), "document-edit-symbolic");
        rename.connect_clicked(emit_paths(state, &popover, path, |s, p| {
            fileops::rename_file(s, &p);
        }));

        let trash = add_context_menu_button(&i18n("Move to trash"), "user-trash-symbolic");
        trash.connect_clicked(emit_paths(state, &popover, path, |s, p| {
            s.sender
                .output(VideoBrowserSidebarOutput::TrashFiles(vec![p]))
                .ok();
        }));
    } else {
        let remove = add_context_menu_button(&i18n("Remove from playlist"), "list-remove-symbolic");
        remove.connect_clicked({
            let state = state.clone();
            let path = path.to_string();
            // WEAK: see the `play` handler — avoid the popover ⇄ item cycle.
            let popover = popover.downgrade();
            move |_| {
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
                remove_from_queue(&state, &path);
            }
        });
    }

    let open_folder = add_context_menu_button(&i18n("Open containing folder"), "folder-symbolic");
    open_folder.connect_clicked(emit_paths(state, &popover, path, |_, p| {
        big_app_kit::desktop::open_containing_folder(std::path::Path::new(&p));
    }));

    popover.set_child(Some(&menu));
    // The popover unparents itself once closed, freeing the row anchor.
    popover.connect_closed(gtk::Popover::unparent);
    popover.popup();
}

/// Build a context-menu click handler that closes the popover and runs `action`
/// with the row's path.
fn emit_paths(
    state: &Rc<BrowserState>,
    popover: &gtk::Popover,
    path: &str,
    action: impl Fn(&Rc<BrowserState>, String) + 'static,
) -> impl Fn(&gtk::Button) + 'static {
    let state = state.clone();
    // WEAK: see `build`'s `play` handler — avoid the popover ⇄ item ⇄ closure
    // cycle that leaks the context menu on every right-click.
    let popover = popover.downgrade();
    let path = path.to_string();
    move |_| {
        if let Some(popover) = popover.upgrade() {
            popover.popdown();
        }
        action(&state, path.clone());
    }
}

/// Remove `path` from the play queue and the shell's mpv playlist. Path-keyed so
/// a concurrent removal that shifted positions cannot drop the wrong entry.
pub(crate) fn remove_from_queue(state: &Rc<BrowserState>, path: &str) {
    let removed = {
        let mut queue = state.queue.borrow_mut();
        let Some(pos) = queue.iter().position(|p| p == path) else {
            return;
        };
        queue.remove(pos)
    };
    state.recompute_pos();
    state.rebuild();
    state
        .sender
        .output(VideoBrowserSidebarOutput::RemoveEntry(removed))
        .ok();
}
