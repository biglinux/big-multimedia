//! Save status and normal-close draining. The data store owns generations and
//! persistence; this module only presents acknowledged state on the GTK thread.
use adw::prelude::*;
use super::{Settings, SavePhase};
use crate::i18n::i18n;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Add a visible, retryable saving status. Timer exists only while mapped and
/// announcements change only when the generation/result changes.
pub fn saving_page(settings: &Settings) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder().title(i18n("Saving preferences"))
        .icon_name("document-save-symbolic").build();
    let group = adw::PreferencesGroup::new();
    let row = adw::ActionRow::builder().title(i18n("Save status"))
        .subtitle(i18n("Your preferences are saved automatically.")).build();
    let retry = gtk::Button::with_label(&i18n("Retry saving"));
    retry.set_valign(gtk::Align::Center); retry.set_sensitive(false);
    row.add_suffix(&retry); group.add(&row); page.add(&group);
    let store = settings.clone();
    retry.connect_clicked(move |_| { let _ = store.request_flush(); });
    let timer = Rc::new(RefCell::new(None::<glib::SourceId>));
    let store = settings.clone(); let timer_map = timer.clone();
    let weak_row = row.downgrade(); let weak_retry = retry.downgrade();
    page.connect_map(move |_| {
        if timer_map.borrow().is_some() { return; }
        let store = store.clone(); let row = weak_row.clone(); let retry = weak_retry.clone();
        let mut last = None;
        *timer_map.borrow_mut() = Some(glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let (Some(row), Some(retry)) = (row.upgrade(), retry.upgrade()) else { return glib::ControlFlow::Break; };
            let status = store.save_status();
            if last.as_ref() != Some(&status) {
                row.set_subtitle(&i18n(match status.phase {
                    SavePhase::Saved => "All preferences are saved.",
                    SavePhase::Pending | SavePhase::Saving => "Saving your changes…",
                    SavePhase::DurabilityPending => "Changes were applied, but safe storage is not confirmed. Retry saving.",
                    SavePhase::Failed => "Changes work for this session but could not be saved. Check storage or close other instances, then retry.",
                }));
                retry.set_sensitive(matches!(status.phase, SavePhase::Failed | SavePhase::DurabilityPending));
                last = Some(status);
            }
            glib::ControlFlow::Continue
        }));
    });
    page.connect_unmap(move |_| { if let Some(timer) = timer.borrow_mut().take() { timer.remove(); } });
    page
}

/// Bind normal close to pending writes and explicit consent for unsaved edits.
/// In a shared host, the status covers live documents across mounted players.
/// A failed save is not silently converted into closing a now-hidden window.
pub fn bind_write_drain(root: &impl IsA<gtk::Widget>) {
    let bound=Rc::new(Cell::new(false));
    root.connect_map(move |widget| {
        if bound.get(){return;}
        let Some(window)=widget.root().and_downcast::<gtk::Window>()else{return;};bound.set(true);
        let deciding=Rc::new(Cell::new(false));let discard=Rc::new(Cell::new(false));
        window.connect_close_request(move |window| {
            if deciding.get(){return glib::Propagation::Stop;}
            if big_os_kit::background::pending()>0 {
                deciding.set(true);
                let dialog=adw::AlertDialog::builder().heading(i18n("Finishing pending work"))
                    .body(i18n("Saves and exports are still finishing. The window will close afterwards. Keeping it open does not cancel those operations.")).build();
                dialog.add_response("keep",&i18n("Keep open"));dialog.set_close_response("keep");
                let keep=deciding.clone();dialog.connect_response(None,move|_,_|{keep.set(false);});
                dialog.present(Some(window));
                let weak=window.downgrade();let waiting=deciding.clone();
                let mut hold=window.application().map(|app|app.hold());
                glib::timeout_add_local(std::time::Duration::from_millis(50),move||{
                    if !waiting.get(){hold.take();return glib::ControlFlow::Break;}
                    if big_os_kit::background::pending()==0 {
                        waiting.set(false);dialog.close();
                        if let Some(window)=weak.upgrade(){window.close();}
                        hold.take();glib::ControlFlow::Break
                    }else{glib::ControlFlow::Continue}
                });
                return glib::Propagation::Stop;
            }
            if discard.replace(false) || !unsaved_changes(){return glib::Propagation::Proceed;}
            deciding.set(true);let weak=window.downgrade();let waiting=deciding.clone();let approved=discard.clone();
            let dialog=unsaved_dialog();
            dialog.connect_response(None,move|_,response|{
                waiting.set(false);
                if response=="discard" {approved.set(true);if let Some(window)=weak.upgrade(){window.close();}}
            });
            dialog.present(Some(window));glib::Propagation::Stop
        });
    });
}
fn unsaved_changes()->bool {Settings::any_unsaved() || big_os_kit::deferred::any_unsaved()}
fn unsaved_dialog()->adw::AlertDialog {
    let dialog=adw::AlertDialog::builder().heading(i18n("Some changes are not confirmed as saved"))
        .body(i18n("Keep the player open to review saving errors and retry. Continuing may discard unsaved session changes; it does not mark them as saved.")).build();
    dialog.add_responses(&[("keep",&i18n("Keep open")),("discard",&i18n("Continue without saved changes"))]);
    dialog.set_default_response(Some("keep"));dialog.set_close_response("keep");
    dialog.set_response_appearance("discard",adw::ResponseAppearance::Destructive);dialog
}

/// Run an owned, acknowledged operation off GTK. The operation outlives a
/// disappearing widget; use weak widgets in `done`. No exception path strands
/// a dialog in Saving. Account prompts use the separate service lane.
pub fn acknowledged_task<T: Send + 'static>(
    service: bool,
    work: impl FnOnce() -> std::io::Result<T> + Send + 'static,
    done: impl FnOnce(std::io::Result<T>) + 'static,
) {
    let (tx,rx)=async_channel::bounded(1);
    let job=move || {
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
            .unwrap_or_else(|_| Err(std::io::Error::other("operation interrupted; recovery may be required")));
        let _ = tx.try_send(result);
    };
    let admitted=if service { big_os_kit::background::submit_service(job) } else { big_os_kit::background::submit(job) };
    match admitted {
        Err(error)=>done(Err(error)),
        Ok(())=>{ glib::spawn_future_local(async move {
            let result=rx.recv().await.unwrap_or_else(|_|Err(std::io::Error::other("worker did not confirm completion")));
            done(result);
        }); },
    }
}

/// Small persistence indicator for independent state stores (IPTV/favorites).
/// Polls only while mapped, uses weak widgets, and never displays document data.
pub fn document_status(
    snapshot: impl Fn() -> Option<big_os_kit::deferred::Status> + 'static,
    retry: impl Fn() + 'static,
) -> gtk::Box {
    let root=gtk::Box::new(gtk::Orientation::Horizontal,8);
    let label=gtk::Label::new(None); label.set_wrap(true); label.set_xalign(0.0); label.set_hexpand(true);
    let button=gtk::Button::with_label(&i18n("Retry saving")); button.set_valign(gtk::Align::Center);
    root.append(&label); root.append(&button); button.connect_clicked(move |_|retry());
    let timer=Rc::new(RefCell::new(None::<glib::SourceId>)); let mapped=timer.clone();
    let snapshot=Rc::new(snapshot); let weak_label=label.downgrade(); let weak_button=button.downgrade();
    root.connect_map(move |_| {
        if mapped.borrow().is_some() { return; }
        let get=snapshot.clone(); let weak_label=weak_label.clone(); let weak_button=weak_button.clone(); let mut last=None;
        *mapped.borrow_mut()=Some(glib::timeout_add_local(std::time::Duration::from_millis(250),move || {
            let (Some(label),Some(button))=(weak_label.upgrade(),weak_button.upgrade()) else { return glib::ControlFlow::Break; };
            let status=get();
            if status!=last {
                use big_os_kit::deferred::Phase;
                label.set_label(&i18n(match status.as_ref().map(|s|s.phase) {
                    None|Some(Phase::Saved)=>"Changes are saved.",
                    Some(Phase::Pending|Phase::Saving)=>"Saving your changes…",
                    Some(Phase::DurabilityPending)=>"Changes were applied, but safe storage is not confirmed. Retry saving.",
                    Some(Phase::Failed)=>"Changes work for this session but could not be saved. Check storage or close other instances, then retry.",
                }));
                button.set_sensitive(matches!(status.as_ref().map(|s|s.phase),Some(Phase::Failed|Phase::DurabilityPending)));
                last=status;
            }
            glib::ControlFlow::Continue
        }));
    });
    root.connect_unmap(move |_| { if let Some(id)=timer.borrow_mut().take() { id.remove(); } }); root
}

/// Defer an explicit quit until admitted work ends; failures require consent.
pub fn quit_after_writes(app: &gio::Application) {
    let app=app.clone(); after_writes(move||app.quit());
}

/// Drain normal system actions and ask before leaving unconfirmed local edits.
/// Forced termination and hosts bypassing this helper are outside this contract.
pub fn after_writes(action:impl FnOnce()+'static) { after_writes_inner(Box::new(action),true); }
fn after_writes_inner(action:Box<dyn FnOnce()>,confirm:bool) {
    let mut action=Some(action);
    let mut hold=gio::Application::default().map(|app|app.hold());
    glib::timeout_add_local(std::time::Duration::from_millis(50),move||{
        if big_os_kit::background::pending()!=0{return glib::ControlFlow::Continue;}
        let Some(action)=action.take()else{return glib::ControlFlow::Break;};
        if confirm && unsaved_changes() {
            let parent=gio::Application::default().and_downcast::<gtk::Application>().and_then(|app|app.active_window());
            if let Some(parent)=parent {
                let dialog=unsaved_dialog();let action=Rc::new(RefCell::new(Some(action)));
                dialog.connect_response(None,move|_,response|{
                    if response=="discard" {if let Some(action)=action.borrow_mut().take(){after_writes_inner(action,false);}}
                });
                dialog.present(Some(&parent));
            }else{log::warn!("system action cancelled: unconfirmed settings and no confirmation window");}
        }else{action();}
        hold.take();glib::ControlFlow::Break
    });
}
