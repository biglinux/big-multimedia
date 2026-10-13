//! Bounded review of unavailable files. No file deletion, no implicit selection,
//! no destructive result inferred from an offline disk or a failed SQL read.
use crate::db::{
    DbHandle,
    unavailable::{self, Availability, ReviewItem},
};
use crate::i18n::i18n;
use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct PageState {
    cursor: i64,
    upper: Option<i64>,
    next: Option<i64>,
    entries: Vec<(ReviewItem, gtk::CheckButton)>,
}

#[derive(Clone)]
struct View {
    state: Rc<RefCell<PageState>>,
    busy: Rc<Cell<bool>>,
    cancel: Arc<AtomicBool>,
    db: DbHandle,
    status: glib::WeakRef<gtk::Label>,
    list: glib::WeakRef<gtk::ListBox>,
    next: glib::WeakRef<gtk::Button>,
    remove: glib::WeakRef<gtk::Button>,
    parent: glib::WeakRef<gtk::Window>,
    changed: Rc<dyn Fn()>,
}

pub(super) fn show(parent: &gtk::Window, db: DbHandle, on_changed: impl Fn() + 'static) {
    let dialog = adw::Dialog::builder()
        .title(i18n("Review unavailable files"))
        .content_width(640)
        .content_height(560)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_top(16);
    root.set_margin_bottom(16);
    root.set_margin_start(16);
    root.set_margin_end(16);
    let description = gtk::Label::new(Some(&i18n(
        "Reconnect external drives before reviewing. Removing records forgets their ratings and saved playlist links, but never deletes media files or interrupts current playback.",
    )));
    description.set_wrap(true);
    description.set_xalign(0.0);
    root.append(&description);
    let status = gtk::Label::new(Some(&i18n("Checking the library…")));
    status.set_wrap(true);
    status.set_xalign(0.0);
    root.append(&status);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_child(Some(&list));
    root.append(&scroll);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let restart = gtk::Button::with_label(&i18n("Restart review"));
    let refresh = gtk::Button::with_label(&i18n("Check again"));
    let next = gtk::Button::with_label(&i18n("Continue checking"));
    let remove = gtk::Button::with_label(&i18n("Remove selected records"));
    remove.add_css_class("destructive-action");
    actions.append(&restart);
    actions.append(&refresh);
    actions.append(&next);
    root.append(&actions);
    root.append(&remove);
    let close = gtk::Button::with_label(&i18n("Close"));
    root.append(&close);
    let weak = dialog.downgrade();
    close.connect_clicked(move |_| {
        if let Some(dialog) = weak.upgrade() {
            dialog.close();
        }
    });
    dialog.set_child(Some(&root));
    let view = View {
        state: Rc::new(RefCell::new(PageState::default())),
        busy: Rc::new(Cell::new(false)),
        cancel: Arc::new(AtomicBool::new(false)),
        db,
        status: status.downgrade(),
        list: list.downgrade(),
        next: next.downgrade(),
        remove: remove.downgrade(),
        parent: parent.downgrade(),
        changed: Rc::new(on_changed),
    };
    let v = view.clone();
    restart.connect_clicked(move |_| {
        if !v.busy.get() {
            v.state.borrow_mut().upper = None;
            v.load(0);
        }
    });
    let v = view.clone();
    refresh.connect_clicked(move |_| {
        let cursor = v.state.borrow().cursor;
        v.load(cursor);
    });
    let v = view.clone();
    next.connect_clicked(move |_| {
        let cursor = v.state.borrow().next;
        if let Some(cursor) = cursor {
            v.load(cursor);
        }
    });
    let v = view.clone();
    remove.connect_clicked(move |_| v.confirm_removal());
    let cancel = view.cancel.clone();
    dialog.connect_closed(move |_| {
        cancel.store(true, Ordering::Release);
    });
    dialog.present(Some(parent));
    view.load(0);
}

impl View {
    fn message(&self, text: &str) {
        if let Some(status) = self.status.upgrade() {
            status.set_text(text);
        }
    }
    fn busy(&self, busy: bool) {
        self.busy.set(busy);
        if let Some(next) = self.next.upgrade() {
            next.set_sensitive(!busy && self.state.borrow().next.is_some());
        }
        if let Some(remove) = self.remove.upgrade() {
            remove.set_sensitive(
                !busy
                    && self
                        .state
                        .borrow()
                        .entries
                        .iter()
                        .any(|(item, _)| item.availability == Availability::Missing),
            );
        }
    }
    fn load(&self, cursor: i64) {
        if self.cancel.load(Ordering::Acquire) || self.busy.get() {
            return;
        }
        self.busy(true);
        self.message(&i18n("Checking the library…"));
        let db = self.db.clone();
        let cancel = self.cancel.clone();
        let upper = self.state.borrow().upper;
        let view = self.clone();
        crate::settings::acknowledged_task(
            true,
            move || unavailable::page(&db, cursor, upper, &cancel).map_err(std::io::Error::other),
            move |result| {
                if view.cancel.load(Ordering::Acquire) {
                    return;
                }
                match result {
                    Ok(page) => view.show_page(cursor, page),
                    Err(_) => view.message(&i18n(
                        "The library could not be checked completely. Previous results are kept. Check the drive or database and retry.",
                    )),
                }
                view.busy(false);
            },
        );
    }
    fn show_page(&self, cursor: i64, page: unavailable::ReviewPage) {
        let Some(list) = self.list.upgrade() else {
            return;
        };
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let count = page.entries.len();
        let mut state = self.state.borrow_mut();
        state.entries.clear();
        state.cursor = cursor;
        state.upper = Some(page.upper_id);
        state.next = page.next_after;
        for item in page.entries {
            let check = review_row(&list, &item);
            state.entries.push((item, check));
        }
        drop(state);
        let progress = if page.next_after.is_none() {
            i18n("End of this review snapshot. Restart to include new imports.")
        } else {
            i18n("Continue to check the next part of your library.")
        };
        self.message(&format!(
            "{} {} · {} {}. {}",
            page.checked,
            i18n("records checked"),
            count,
            i18n("records need review"),
            progress
        ));
    }
    fn confirm_removal(&self) {
        if self.busy.get() || self.cancel.load(Ordering::Acquire) {
            return;
        }
        let selected: Vec<_> = self
            .state
            .borrow()
            .entries
            .iter()
            .filter(|(item, check)| item.availability == Availability::Missing && check.is_active())
            .map(|(item, _)| item.clone())
            .collect();
        if selected.is_empty() {
            self.message(&i18n("Select the missing records you want to remove."));
            return;
        }
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let body = format!(
            "{}: {}. {}",
            i18n("Selected records"),
            selected.len(),
            i18n(
                "Their ratings and saved playlist links will be removed. Media files will not be deleted. A restored file or changed record cancels the whole selection.",
            )
        );
        let confirm = adw::AlertDialog::builder()
            .heading(i18n("Remove selected library records?"))
            .body(body)
            .build();
        confirm.add_responses(&[
            ("cancel", &i18n("Cancel")),
            ("remove", &i18n("Remove records")),
        ]);
        confirm.set_default_response(Some("cancel"));
        confirm.set_close_response("cancel");
        confirm.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        self.busy(true);
        let view = self.clone();
        confirm.connect_response(None, move |_, response| {
            if response != "remove" || view.cancel.load(Ordering::Acquire) {
                view.busy(false);
                return;
            }
            view.message(&i18n("Rechecking the selection and updating the library…"));
            let db = view.db.clone();
            let cancel = view.cancel.clone();
            let selected = selected.clone();
            let view = view.clone();
            crate::settings::acknowledged_task(
                false,
                move || {
                    unavailable::remove_reviewed(&db, &selected, &cancel)
                        .map_err(std::io::Error::other)
                },
                move |result| {
                    if result.is_ok() {
                        (view.changed)();
                    }
                    if view.cancel.load(Ordering::Acquire) {
                        return;
                    }
                    view.busy(false);
                    match result {
                        Ok(_) => {
                            view.state.borrow_mut().upper = None;
                            view.load(0);
                        }
                        Err(_) => view.message(&i18n(
                            "Removal was not confirmed. A file may be available again or a record changed. Check again before selecting records.",
                        )),
                    }
                },
            );
        });
        confirm.present(Some(&parent));
    }
}

/// Append one review row to `list` and return its selection check button.
fn review_row(list: &gtk::ListBox, item: &ReviewItem) -> gtk::CheckButton {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.set_margin_top(8);
    row.set_margin_bottom(8);
    row.set_margin_start(10);
    row.set_margin_end(10);
    let check = gtk::CheckButton::new();
    check.set_sensitive(item.availability == Availability::Missing);
    check.set_valign(gtk::Align::Center);
    check.update_property(&[gtk::accessible::Property::Label(&format!(
        "{} {}",
        i18n("Select"),
        item.track.display_title()
    ))]);
    row.append(&check);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(item.track.display_title()));
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    text.append(&title);
    let path_text = if item.availability == Availability::NotLocal {
        i18n("Remote or unsupported location — not eligible for removal here")
    } else {
        item.track.file_path.clone()
    };
    let path = gtk::Label::new(Some(&path_text));
    path.set_xalign(0.0);
    path.set_ellipsize(pango::EllipsizeMode::Middle);
    path.add_css_class("dim-label");
    text.append(&path);
    let reason = i18n(match item.availability {
        Availability::Missing => {
            "Not found — select only if you want to forget this library record"
        }
        Availability::NoAccess => {
            "Access could not be verified — check permissions before reviewing"
        }
        Availability::NotRegular => "This location is not a regular media file",
        Availability::NotLocal => "Remote items are not checked as local files",
        Availability::Present => "Available again",
    });
    let detail = gtk::Label::new(Some(&format!(
        "{reason} · {}: {} · {}: {}",
        i18n("Rating"),
        item.track.rating,
        i18n("Saved playlists"),
        item.playlist_count
    )));
    detail.set_wrap(true);
    detail.set_xalign(0.0);
    text.append(&detail);
    row.append(&text);
    list.append(&row);
    check
}
