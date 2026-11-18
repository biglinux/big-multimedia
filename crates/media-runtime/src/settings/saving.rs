//! Save status and normal-close draining. The data store owns generations and
//! persistence; this module only presents acknowledged state on the GTK thread.
use super::{SavePhase, Settings};
use crate::i18n::i18n;
use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Put a save-problem banner on `page`. It stays hidden while saving works and
/// appears with a retry action only when the store reports a failed or
/// unconfirmed write.
pub fn attach_save_status_banner(page: &adw::PreferencesPage, settings: &Settings) {
    let banner = adw::Banner::builder()
        .button_label(i18n("Retry Saving"))
        .revealed(false)
        .build();
    page.set_banner(Some(&banner));
    let store = settings.clone();
    banner.connect_button_clicked(move |_| {
        let _ = store.request_flush();
    });
    let store = settings.clone();
    let weak_banner = banner.downgrade();
    let mut last = None;
    refresh_while_mapped(page, move || {
        let Some(banner) = weak_banner.upgrade() else {
            return;
        };
        let status = store.save_status();
        if last.as_ref() != Some(&status) {
            let problem = match status.phase {
                SavePhase::Failed => Some(i18n(
                    "Your changes work for now but could not be saved. Check free disk space or close other players, then retry.",
                )),
                SavePhase::Saved | SavePhase::Pending | SavePhase::Saving => None,
            };
            if let Some(problem) = &problem {
                banner.set_title(problem);
            }
            banner.set_revealed(problem.is_some());
            last = Some(status);
        }
    });
}

/// Call `refresh` when `widget` maps and after each background save change
/// until it unmaps, so nothing runs while saves are idle.
fn refresh_while_mapped(widget: &impl IsA<gtk::Widget>, refresh: impl FnMut() + 'static) {
    let refresh = Rc::new(RefCell::new(refresh));
    let watch = Rc::new(RefCell::new(None::<big_os_kit::background::Watch>));
    let mapped = watch.clone();
    widget.connect_map(move |_| {
        if mapped.borrow().is_some() {
            return;
        }
        let (wake, changes) = async_channel::bounded(1);
        *mapped.borrow_mut() = Some(big_os_kit::background::watch(move || {
            let _ = wake.try_send(());
        }));
        (refresh.borrow_mut())();
        let refresh = refresh.clone();
        // Ends when unmap drops the watch, which closes the channel.
        glib::spawn_future_local(async move {
            while changes.recv().await.is_ok() {
                (refresh.borrow_mut())();
            }
        });
    });
    widget.connect_unmap(move |_| {
        watch.borrow_mut().take();
    });
}

/// Bind normal close to pending writes and explicit consent for unsaved edits.
/// In a shared host, the status covers live documents across mounted players.
/// A failed save is not silently converted into closing a now-hidden window.
pub fn bind_write_drain(root: &impl IsA<gtk::Widget>) {
    bind_write_drain_with_unsaved(root, || false);
}

/// Include an independently acknowledged store in normal-close protection.
pub fn bind_write_drain_with_unsaved(
    root: &impl IsA<gtk::Widget>,
    extra_unsaved: impl Fn() -> bool + 'static,
) {
    let extra_unsaved = Rc::new(extra_unsaved);
    let bound = Rc::new(Cell::new(false));
    root.connect_map(move |widget| {
        if bound.get() {
            return;
        }
        let Some(window) = widget.root().and_downcast::<gtk::Window>() else {
            return;
        };
        bound.set(true);
        let deciding = Rc::new(Cell::new(false));
        let discard = Rc::new(Cell::new(false));
        let extra_unsaved = extra_unsaved.clone();
        window.connect_close_request(move |window| {
            if deciding.get() {
                return glib::Propagation::Stop;
            }
            if big_os_kit::background::pending() > 0 {
                deciding.set(true);
                wait_for_pending_work(window, &deciding);
                return glib::Propagation::Stop;
            }
            if discard.replace(false) || !(unsaved_changes() || extra_unsaved()) {
                return glib::Propagation::Proceed;
            }
            deciding.set(true);
            let weak = window.downgrade();
            let waiting = deciding.clone();
            let approved = discard.clone();
            let dialog = unsaved_dialog();
            dialog.connect_response(None, move |_, response| {
                waiting.set(false);
                if response == "discard" {
                    approved.set(true);
                    if let Some(window) = weak.upgrade() {
                        window.close();
                    }
                }
            });
            dialog.present(Some(window));
            glib::Propagation::Stop
        });
    });
}

/// Tell the person saves are finishing and close `window` once they are,
/// unless they choose to keep it open.
fn wait_for_pending_work(window: &gtk::Window, deciding: &Rc<Cell<bool>>) {
    let body = i18n(
        "Saves and exports are still finishing. The window will close afterwards. Keeping it open does not cancel those operations.",
    );
    let dialog = big_gtk_kit::alert::Alert::builder()
        .heading(i18n("Finishing pending work"))
        .body(body)
        .build();
    dialog.add_response("keep", &i18n("Keep open"));
    dialog.set_close_response("keep");
    let (wake, changes) = async_channel::bounded(1);
    let keep = deciding.clone();
    let kept = wake.clone();
    dialog.connect_response(None, move |_, _| {
        keep.set(false);
        let _ = kept.try_send(());
    });
    dialog.present(Some(window));
    let watch = big_os_kit::background::watch(move || {
        let _ = wake.try_send(());
    });
    let weak = window.downgrade();
    let waiting = deciding.clone();
    let hold = window.application().map(|app| app.hold());
    glib::spawn_future_local(async move {
        let _watch = watch;
        let _hold = hold;
        while waiting.get() && big_os_kit::background::pending() != 0 {
            let _ = changes.recv().await;
        }
        if waiting.replace(false) {
            dialog.close();
            if let Some(window) = weak.upgrade() {
                window.close();
            }
        }
    });
}

fn unsaved_changes() -> bool {
    Settings::any_unsaved() || big_os_kit::deferred::any_unsaved()
}
fn unsaved_dialog() -> big_gtk_kit::alert::Alert {
    let body = i18n(
        "Keep the player open to review saving errors and retry. Continuing may discard unsaved session changes; it does not mark them as saved.",
    );
    let dialog = big_gtk_kit::alert::Alert::builder()
        .heading(i18n("Some changes are not confirmed as saved"))
        .body(body)
        .build();
    dialog.add_responses(&[
        ("keep", &i18n("Keep open")),
        ("discard", &i18n("Continue without saved changes")),
    ]);
    dialog.set_default_response(Some("keep"));
    dialog.set_close_response("keep");
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    dialog
}

/// Run an owned, acknowledged operation off GTK. The operation outlives a
/// disappearing widget; use weak widgets in `done`. No exception path strands
/// a dialog in Saving. Keyring prompts use the separate service lane.
pub fn acknowledged_task<T: Send + 'static>(
    service: bool,
    work: impl FnOnce() -> std::io::Result<T> + Send + 'static,
    done: impl FnOnce(std::io::Result<T>) + 'static,
) {
    let (tx, rx) = async_channel::bounded(1);
    let job = move || {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| {
                Err(std::io::Error::other(
                    "operation interrupted; recovery may be required",
                ))
            });
        let _ = tx.try_send(result);
    };
    let admitted = if service {
        big_os_kit::background::submit_service(job)
    } else {
        big_os_kit::background::submit(job)
    };
    match admitted {
        Err(error) => done(Err(error)),
        Ok(()) => {
            glib::spawn_future_local(async move {
                let result = rx.recv().await.unwrap_or_else(|_| {
                    Err(std::io::Error::other("worker did not confirm completion"))
                });
                done(result);
            });
        }
    }
}

/// Small persistence indicator for independent state stores (IPTV/favorites).
/// Refreshes only while mapped, uses weak widgets, and never displays document data.
pub fn document_status(
    snapshot: impl Fn() -> Option<big_os_kit::deferred::Status> + 'static,
    retry: impl Fn() + 'static,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(None);
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.set_hexpand(true);
    let button = gtk::Button::with_label(&i18n("Retry saving"));
    button.set_valign(gtk::Align::Center);
    root.append(&label);
    root.append(&button);
    button.connect_clicked(move |_| retry());
    let weak_label = label.downgrade();
    let weak_button = button.downgrade();
    let mut last = None;
    refresh_while_mapped(&root, move || {
        let (Some(label), Some(button)) = (weak_label.upgrade(), weak_button.upgrade()) else {
            return;
        };
        let status = snapshot();
        if status != last {
            use big_os_kit::deferred::Phase;
            let phase = status.as_ref().map(|s| s.phase);
            label.set_label(&i18n(match phase {
                None | Some(Phase::Saved) => "Changes are saved.",
                Some(Phase::Pending | Phase::Saving) => "Saving your changes…",
                Some(Phase::Failed) => {
                    "Changes work for this session but could not be saved. Check storage or close other instances, then retry."
                }
            }));
            button.set_sensitive(matches!(phase, Some(Phase::Failed)));
            last = status;
        }
    });
    root
}

/// Defer an explicit quit until admitted work ends; failures require consent.
pub fn quit_after_writes(app: &gio::Application) {
    let app = app.clone();
    after_writes(move || app.quit());
}

/// Drain normal system actions and ask before leaving unconfirmed local edits.
/// Forced termination and hosts bypassing this helper are outside this contract.
pub fn after_writes(action: impl FnOnce() + 'static) {
    after_writes_inner(Box::new(action), true);
}
fn after_writes_inner(action: Box<dyn FnOnce()>, confirm: bool) {
    let hold = gio::Application::default().map(|app| app.hold());
    let (wake, changes) = async_channel::bounded(1);
    let watch = big_os_kit::background::watch(move || {
        let _ = wake.try_send(());
    });
    glib::spawn_future_local(async move {
        let _hold = hold;
        while big_os_kit::background::pending() != 0 {
            let _ = changes.recv().await;
        }
        drop(watch);
        if confirm && unsaved_changes() {
            let parent = gio::Application::default()
                .and_downcast::<gtk::Application>()
                .and_then(|app| app.active_window());
            if let Some(parent) = parent {
                let dialog = unsaved_dialog();
                let action = Rc::new(RefCell::new(Some(action)));
                dialog.connect_response(None, move |_, response| {
                    if response == "discard"
                        && let Some(action) = action.borrow_mut().take()
                    {
                        after_writes_inner(action, false);
                    }
                });
                dialog.present(Some(&parent));
            } else {
                log::warn!(
                    "system action cancelled: unconfirmed settings and no confirmation window"
                );
            }
        } else {
            action();
        }
    });
}
