//! Progress reflects committed imports; closing the dialog requests cancellation.
use crate::audio::library_scan_engine::ScanControl;
use crate::i18n::i18n;
use adw::prelude::*;
use big_relm4_components::layout::dialog_page::BigDialogPage;
use std::sync::{Arc, atomic::Ordering};

pub(super) fn show(parent: &gtk::Window, control: Arc<ScanControl>) {
    let dialog = adw::Dialog::builder()
        .title(i18n("Scanning the library"))
        .content_width(440)
        .build();
    let page = BigDialogPage::plain();
    let content = page.content().clone();
    content.set_spacing(18);
    content.add_css_class("scan-progress");

    // A spinner while it reads, then an icon that says how it ended.
    let spinner = adw::Spinner::builder()
        .width_request(48)
        .height_request(48)
        .halign(gtk::Align::Center)
        .build();
    let outcome = gtk::Image::builder()
        .pixel_size(48)
        .halign(gtk::Align::Center)
        .visible(false)
        .css_classes(["scan-outcome"])
        .build();
    content.append(&spinner);
    content.append(&outcome);

    let status = gtk::Label::builder()
        .label(i18n("Reading media information…"))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["title-3"])
        .build();
    content.append(&status);

    let stats = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .homogeneous(true)
        .build();
    let examined = stat_card(&stats, &i18n("Examined"));
    let imported = stat_card(&stats, &i18n("Imported"));
    let skipped = stat_card(&stats, &i18n("Skipped"));
    content.append(&stats);

    let explanation = gtk::Label::builder()
        .label(i18n(
            "You can keep using the player. Cancelling keeps earlier confirmed imports and never deletes media files.",
        ))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["caption", "dim-label"])
        .build();
    content.append(&explanation);

    let cancel = gtk::Button::builder()
        .label(i18n("Cancel scan"))
        .halign(gtk::Align::Center)
        .css_classes(["pill"])
        .build();
    content.append(&cancel);
    dialog.set_child(Some(page.root()));

    let revoke = control.clone();
    let close_dialog = dialog.downgrade();
    cancel.connect_clicked(move |button| {
        if revoke.finished.load(Ordering::Acquire) {
            if let Some(dialog) = close_dialog.upgrade() {
                dialog.close();
            }
            return;
        }
        revoke.cancelled.store(true, Ordering::Release);
        button.set_sensitive(false);
    });
    let revoke = control.clone();
    dialog.connect_closed(move |_| {
        if !revoke.finished.load(Ordering::Acquire) {
            revoke.cancelled.store(true, Ordering::Release);
        }
    });
    let weak_status = status.downgrade();
    let weak_counts = StatLabels {
        examined: examined.downgrade(),
        imported: imported.downgrade(),
        skipped: skipped.downgrade(),
    };
    let weak_cancel = cancel.downgrade();
    let weak_spinner = spinner.downgrade();
    let weak_outcome = outcome.downgrade();
    let timer = std::rc::Rc::new(std::cell::RefCell::new(None::<glib::SourceId>));
    let mapped = timer.clone();
    // Polls the worker's counters while the dialog is mapped and stops once the
    // scan has finished, so a finished dialog left open costs no wakeups.
    content.connect_map(move |_| {
        if mapped.borrow().is_some() {
            return;
        }
        let control = control.clone();
        let status = weak_status.clone();
        let counts = weak_counts.clone();
        let cancel = weak_cancel.clone();
        let spinner = weak_spinner.clone();
        let outcome = weak_outcome.clone();
        let mut last = None;
        let tick = move || {
            let (Some(status), Some(counts), Some(cancel), Some(spinner), Some(outcome)) = (
                status.upgrade(),
                counts.upgrade(),
                cancel.upgrade(),
                spinner.upgrade(),
                outcome.upgrade(),
            ) else {
                return glib::ControlFlow::Break;
            };
            let values = (
                control.examined.load(Ordering::Acquire),
                control.imported.load(Ordering::Acquire),
                control.skipped.load(Ordering::Acquire),
                control.cancelled.load(Ordering::Acquire),
                control.finished.load(Ordering::Acquire),
                control.failed.load(Ordering::Acquire),
            );
            if last == Some(values) {
                return glib::ControlFlow::Continue;
            }
            last = Some(values);
            let (examined, imported, skipped, cancelled, finished, failed) = values;
            counts.examined.set_text(&examined.to_string());
            counts.imported.set_text(&imported.to_string());
            counts.skipped.set_text(&skipped.to_string());
            let message = if finished && cancelled {
                "Scan cancelled. Earlier confirmed imports were kept."
            } else if finished && failed {
                "Scan incomplete. Earlier confirmed imports were kept; check storage and retry."
            } else if finished {
                "Library scan finished."
            } else if cancelled {
                "Cancellation requested. Finishing the current safe step…"
            } else {
                "Reading media information…"
            };
            status.set_text(&i18n(message));
            cancel.set_label(&i18n(if finished { "Close" } else { "Cancel scan" }));
            cancel.set_sensitive(finished || !cancelled);
            if finished {
                cancel.add_css_class("suggested-action");
                let (icon, class) = if cancelled {
                    ("process-stop-symbolic", "dim-label")
                } else if failed {
                    ("dialog-warning-symbolic", "warning")
                } else {
                    ("object-select-symbolic", "success")
                };
                outcome.set_icon_name(Some(icon));
                outcome.add_css_class(class);
                outcome.set_visible(true);
                spinner.set_visible(false);
            }
            if finished {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        };
        let source = glib::timeout_add_local(std::time::Duration::from_millis(250), tick);
        *mapped.borrow_mut() = Some(source);
    });
    content.connect_unmap(move |_| {
        if let Some(timer) = timer.borrow_mut().take() {
            big_gtk_kit::source::remove(timer);
        }
    });
    dialog.present(Some(parent));
}

/// One count under the status: a large number over its name.
fn stat_card(row: &gtk::Box, name: &str) -> gtk::Label {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .css_classes(["card", "scan-stat"])
        .build();
    let value = gtk::Label::builder()
        .label("0")
        .css_classes(["scan-stat-value", "numeric"])
        .build();
    let caption = gtk::Label::builder()
        .label(name)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["caption", "dim-label"])
        .build();
    card.append(&value);
    card.append(&caption);
    row.append(&card);
    value
}

/// The count labels, held weakly by the polling timer.
#[derive(Clone)]
struct StatLabels {
    examined: glib::WeakRef<gtk::Label>,
    imported: glib::WeakRef<gtk::Label>,
    skipped: glib::WeakRef<gtk::Label>,
}

/// The live labels for one tick.
struct StatValues {
    examined: gtk::Label,
    imported: gtk::Label,
    skipped: gtk::Label,
}

impl StatLabels {
    fn upgrade(&self) -> Option<StatValues> {
        Some(StatValues {
            examined: self.examined.upgrade()?,
            imported: self.imported.upgrade()?,
            skipped: self.skipped.upgrade()?,
        })
    }
}
