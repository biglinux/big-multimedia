//! Reviewed account edit. Secret values stay in password rows and worker payloads,
//! never in diagnostics. Saving preserves the existing history-sharing consent.
use adw::prelude::*;
use std::sync::{Arc, atomic::{AtomicBool,Ordering}};
use std::rc::Rc;
use std::cell::{Cell,RefCell};
use crate::i18n::i18n;
use big_media_runtime::library::accounts::{self,AccountReview,Provider,SaveOutcome};

pub(super) fn show(parent:&gtk::Window,provider:Provider,review:AccountReview,on_finished:impl Fn()+'static) {
    let recovering=review.kind==accounts::StorageKind::RecoveryRequired;
    let dialog=adw::Dialog::builder().title(i18n("Account credentials")).content_width(480).build();
    let root=gtk::Box::new(gtk::Orientation::Vertical,12);root.set_margin_top(16);root.set_margin_bottom(16);root.set_margin_start(16);root.set_margin_end(16);
    let note=gtk::Label::new(Some(&i18n("Credentials are saved only in your desktop keyring. Existing listening-history consent is preserved; this does not activate an unavailable integration.")));
    note.set_wrap(true);note.set_xalign(0.0);root.append(&note);
    let status=gtk::Label::new(Some(&i18n("Waiting for your keyring. Unlock it if prompted…")));status.set_wrap(true);status.set_xalign(0.0);root.append(&status);
    let fields:&[(&str,&str)]=match provider {
        Provider::LastFm=>&[("api_key","API key"),("api_secret","API secret"),("session_key","Session key"),("username","Username")],
        Provider::ListenBrainz=>&[("user_token","User token")],
    };
    let group=adw::PreferencesGroup::new();let mut rows=Vec::new();
    for &(key,title) in fields {
        let row=adw::PasswordEntryRow::builder().title(i18n(title)).build();
        row.set_sensitive(false);group.add(&row);rows.push((key.to_owned(),row));
    }
    root.append(&group);
    let save=gtk::Button::with_label(&i18n("Save in keyring"));save.add_css_class("suggested-action");save.set_sensitive(false);root.append(&save);
    let close=gtk::Button::with_label(&i18n("Close"));root.append(&close);
    let weak=dialog.downgrade();close.connect_clicked(move |_|{if let Some(dialog)=weak.upgrade(){dialog.close();}});
    dialog.set_child(Some(&root));dialog.present(Some(parent));
    let cancelled=Arc::new(AtomicBool::new(false));let revoke=cancelled.clone();dialog.connect_closed(move |_|{revoke.store(true,Ordering::Release);});
    let values=Rc::new(RefCell::new(rows));let enabled=Rc::new(Cell::new(false));let busy=Rc::new(Cell::new(true));
    let reviewed=Rc::new(review);let finish=Rc::new(on_finished);
    let weak_status=status.downgrade();let weak_save=save.downgrade();let weak_dialog=dialog.downgrade();
    let view_rows=values.clone();let consent=enabled.clone();let working=busy.clone();let cancel=cancelled.clone();let worker_review=(*reviewed).clone();
    crate::settings::acknowledged_task(true,move||accounts::editor_seed(&worker_review,&cancel),move|result|{
        working.set(false);let (Some(status),Some(save))=(weak_status.upgrade(),weak_save.upgrade())else{return;};
        let payload=match result {Ok(payload)=>payload,Err(_)=>{status.set_text(&i18n("Credentials could not be read safely. Unlock the keyring and reopen this editor."));return;}};
        let data=payload.as_deref().map(serde_json::from_slice::<serde_json::Value>).transpose();
        let data=match data {Ok(data)=>data.unwrap_or_else(||serde_json::json!({"enabled":false})),Err(_)=>{status.set_text(&i18n("Saved credentials are invalid. No changes were made."));return;}};
        consent.set(data["enabled"].as_bool().unwrap_or(false));
        for (key,row) in view_rows.borrow().iter(){row.set_text(data[key.as_str()].as_str().unwrap_or(""));row.set_sensitive(true);}
        status.set_text(&i18n(if recovering {"A save is pending. Re-enter exactly the intended credentials to retry that same transaction, or close and use Recover account. A different payload will be refused."} else {"Edit the credentials, then save. Closing without saving discards your edits."}));save.set_sensitive(true);
    });
    let rows=values;let consent=enabled;let working=busy;let weak_status=status.downgrade();
    save.connect_clicked(move|button|{
        if working.replace(true){return;}
        let mut payload=serde_json::Map::new();
        for (key,row) in rows.borrow().iter(){
            let value=row.text();
            if value.is_empty()||value.len()>4096||value.chars().any(char::is_control){
                working.set(false);if let Some(status)=weak_status.upgrade(){status.set_text(&i18n("Fill every credential field with a valid value."));}return;
            }
            payload.insert(key.clone(),serde_json::Value::String(value.to_string()));
        }
        payload.insert("enabled".into(),serde_json::Value::Bool(consent.get()));
        let bytes=match serde_json::to_vec(&payload){Ok(v) if v.len()<=big_os_kit::keyring::MAX_SECRET_BYTES=>v,_=>{
            working.set(false);if let Some(status)=weak_status.upgrade(){status.set_text(&i18n("The combined credentials are too large for the keyring helper."));}return;
        }};
        button.set_sensitive(false);for (_,row) in rows.borrow().iter(){row.set_sensitive(false);}
        if let Some(status)=weak_status.upgrade(){status.set_text(&i18n("Waiting for your keyring. Unlock it if prompted…"));}
        let review=(*reviewed).clone();let cancel=cancelled.clone();let done=finish.clone();let status=weak_status.clone();let dialog=weak_dialog.clone();
        crate::settings::acknowledged_task(true,move||accounts::save_reviewed(&review,&bytes,&cancel),move|result|{
            done();
            if let Some(status)=status.upgrade(){
                status.set_text(&i18n(match &result {
                    Ok(SaveOutcome::Protected)=>"Credentials are protected in your keyring.",
                    Ok(SaveOutcome::ProtectedCleanupPending)=>"Credentials are protected, but cleanup needs recovery. Close this editor and use Recover account.",
                    Err(_)=>"Saving was not fully confirmed. Close this editor, reload and recover the pending operation. No plaintext fallback was used.",
                }));
            }
            if matches!(&result,Ok(SaveOutcome::Protected)){if let Some(dialog)=dialog.upgrade(){dialog.close();}}
        });
    });
}
