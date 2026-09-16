//! Progress reflects committed imports; closing the dialog requests cancellation.
use adw::prelude::*;
use std::sync::{Arc, atomic::Ordering};
use crate::audio::library_scan_engine::ScanControl;
use crate::i18n::i18n;

pub(super) fn show(parent: &gtk::Window, control: Arc<ScanControl>) {
    let dialog = adw::Dialog::builder().title(i18n("Scanning the library")).content_width(480).build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(20); content.set_margin_bottom(20); content.set_margin_start(20); content.set_margin_end(20);
    let status = gtk::Label::new(Some(&i18n("Reading media information…")));
    status.set_wrap(true); status.set_xalign(0.0); content.append(&status);
    let counts = gtk::Label::new(None); counts.set_xalign(0.0); counts.set_wrap(true); content.append(&counts);
    let explanation = gtk::Label::new(Some(&i18n("You can keep using the player. Cancelling keeps earlier confirmed imports and never deletes media files.")));
    explanation.set_wrap(true); explanation.set_xalign(0.0); content.append(&explanation);
    let cancel = gtk::Button::with_label(&i18n("Cancel scan")); content.append(&cancel);
    let revoke = control.clone(); let close_dialog=dialog.downgrade();
    cancel.connect_clicked(move |button| {
        if revoke.finished.load(Ordering::Acquire) {if let Some(dialog)=close_dialog.upgrade(){dialog.close();}return;}
        revoke.cancelled.store(true, Ordering::Release); button.set_sensitive(false);
    });
    let revoke = control.clone(); dialog.connect_closed(move |_| { if !revoke.finished.load(Ordering::Acquire) { revoke.cancelled.store(true, Ordering::Release); } });
    let weak_status = status.downgrade(); let weak_counts = counts.downgrade(); let weak_cancel = cancel.downgrade();
    let timer = std::rc::Rc::new(std::cell::RefCell::new(None::<glib::SourceId>));
    let mapped = timer.clone();
    content.connect_map(move |_| {
        if mapped.borrow().is_some() { return; }
        let control = control.clone(); let status = weak_status.clone(); let counts = weak_counts.clone(); let cancel = weak_cancel.clone();
        let mut last = None;
        *mapped.borrow_mut() = Some(glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let (Some(status), Some(counts), Some(cancel)) = (status.upgrade(), counts.upgrade(), cancel.upgrade()) else { return glib::ControlFlow::Break; };
            let values = (control.examined.load(Ordering::Acquire), control.imported.load(Ordering::Acquire), control.skipped.load(Ordering::Acquire), control.cancelled.load(Ordering::Acquire), control.finished.load(Ordering::Acquire), control.failed.load(Ordering::Acquire));
            if last != Some(values) {
                let (examined, imported, skipped, cancelled, finished, failed) = values;
                counts.set_text(&format!("{}: {examined} · {}: {imported} · {}: {skipped}", i18n("Examined"), i18n("Imported"), i18n("Skipped")));
                let message = if finished && cancelled { "Scan cancelled. Earlier confirmed imports were kept." }
                    else if finished && failed { "Scan incomplete. Earlier confirmed imports were kept; check storage and retry." }
                    else if finished { "Library scan finished." }
                    else if cancelled { "Cancellation requested. Finishing the current safe step…" }
                    else { "Reading media information…" };
                status.set_text(&i18n(message));
                cancel.set_label(&i18n(if finished {"Close"}else{"Cancel scan"}));
                cancel.set_sensitive(finished||!cancelled); last = Some(values);
            }
            glib::ControlFlow::Continue
        }));
    });
    content.connect_unmap(move |_| { if let Some(timer) = timer.borrow_mut().take() { timer.remove(); } });
    dialog.set_child(Some(&content)); dialog.present(Some(parent));
}
