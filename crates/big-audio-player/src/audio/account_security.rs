//! Keyring migration for Last.fm/ListenBrainz credentials saved by older
//! releases. The player never sends listening history, so the group and each
//! row stay hidden until a provider actually has saved credentials: those can
//! still be moved out of a plaintext file or recovered from an interrupted move.
use crate::i18n::i18n;
use adw::prelude::*;
use big_media_runtime::library::accounts::{
    self, AccountReview, Provider, SaveOutcome, StorageKind,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub(super) fn group() -> adw::PreferencesGroup {
    let group=adw::PreferencesGroup::builder().title(i18n("Saved account security"))
        .description(i18n("Move existing account credentials to your desktop keyring. This does not enable listening-history sharing.")).visible(false).build();
    for (provider, name) in [
        (Provider::LastFm, "Last.fm"),
        (Provider::ListenBrainz, "ListenBrainz"),
    ] {
        let row = adw::ActionRow::builder()
            .title(name)
            .subtitle(i18n("Checking saved credentials…"))
            .visible(false)
            .build();
        let button = gtk::Button::with_label(&i18n("Protect credentials"));
        button.set_valign(gtk::Align::Center);
        button.set_sensitive(false);
        let reload = gtk::Button::with_label(&i18n("Reload"));
        reload.set_valign(gtk::Align::Center);
        row.add_suffix(&reload);
        row.add_suffix(&button);
        group.add(&row);
        let reviewed = Rc::new(RefCell::new(None::<AccountReview>));
        let busy = Rc::new(Cell::new(false));
        let state = reviewed.clone();
        let in_flight = busy.clone();
        let weak_row = row.downgrade();
        let weak_button = button.downgrade();
        reload.connect_clicked(move |_| {
            if let (Some(row), Some(button)) = (weak_row.upgrade(), weak_button.upgrade()) {
                refresh(provider, &row, &button, state.clone(), in_flight.clone());
            }
        });
        let state = reviewed.clone();
        let in_flight = busy.clone();
        let weak_row = row.downgrade();
        button.connect_clicked(move |button| {
            if in_flight.replace(true) { return; }
            let Some(review)=state.borrow().clone() else { in_flight.set(false);return; };
            button.set_sensitive(false);let weak_button=button.downgrade();let weak_row=weak_row.clone();let state=state.clone();let in_flight=in_flight.clone();
            if let Some(row)=weak_row.upgrade() { row.set_subtitle(&i18n("Waiting for your keyring. Unlock it if prompted…")); }
            crate::settings::acknowledged_task(true,move || {
                let cancel=std::sync::atomic::AtomicBool::new(false);
                match review.kind {
                    StorageKind::LegacyFile=>accounts::protect(&review,&cancel),
                    StorageKind::RecoveryRequired=>accounts::recover(&review,&cancel),
                    StorageKind::SecretService=>accounts::load(provider).and_then(|value|value.map(|_|SaveOutcome::Protected)
                        .ok_or_else(||std::io::Error::other("account disappeared"))),
                    StorageKind::NotConfigured=>Err(std::io::Error::other("account not configured")),
                }
            },move |result| {
                in_flight.set(false);
                let (Some(row),Some(button))=(weak_row.upgrade(),weak_button.upgrade()) else {return;};
                match result {
                    Ok(SaveOutcome::Protected)=>refresh(provider,&row,&button,state,in_flight),
                    Ok(SaveOutcome::ProtectedCleanupPending)=>{
                        row.set_subtitle(&i18n("Credentials are protected. Cleanup of an older keyring item needs attention; retry recovery."));
                        button.set_label(&i18n("Recover account"));button.set_sensitive(true);
                        // Reload the exact disk revision before the next attempt.
                        state.borrow_mut().take();
                        refresh(provider,&row,&button,state,in_flight);
                    }
                    Err(_)=>{
                        row.set_subtitle(&i18n("The operation was not fully confirmed. Unlock the keyring, reload and retry recovery. No plaintext fallback was used."));
                        button.set_sensitive(false);state.borrow_mut().take();
                    }
                }
            });
        });
        refresh(provider, &row, &button, reviewed, busy);
    }
    group
}
fn refresh(
    provider: Provider,
    row: &adw::ActionRow,
    button: &gtk::Button,
    state: Rc<RefCell<Option<AccountReview>>>,
    busy: Rc<Cell<bool>>,
) {
    if busy.replace(true) {
        return;
    }
    button.set_sensitive(false);
    let weak_row = row.downgrade();
    let weak_button = button.downgrade();
    crate::settings::acknowledged_task(
        false,
        move || accounts::review(provider),
        move |result| {
            busy.set(false);
            let (Some(row), Some(button)) = (weak_row.upgrade(), weak_button.upgrade()) else {
                return;
            };
            if let Ok(review) = result {
                let (description, action) = match review.kind {
                    StorageKind::NotConfigured => (
                        "No credentials are saved for this account.",
                        "Protect credentials",
                    ),
                    StorageKind::LegacyFile => (
                        "Credentials are still in a private file. Protect them in your desktop keyring.",
                        "Protect credentials",
                    ),
                    StorageKind::SecretService => (
                        "Credentials are stored in the desktop keyring.",
                        "Check keyring access",
                    ),
                    StorageKind::RecoveryRequired => (
                        "An account operation was interrupted. Recover it before making another change.",
                        "Recover account",
                    ),
                };
                row.set_subtitle(&i18n(description));
                button.set_label(&i18n(action));
                button.set_sensitive(review.kind != StorageKind::NotConfigured);
                reveal(&row, review.kind != StorageKind::NotConfigured);
                *state.borrow_mut() = Some(review);
            } else {
                state.borrow_mut().take();
                row.set_subtitle(&i18n("Saved account data could not be read safely. Check permissions and reload; no changes were made."));
                reveal(&row, true);
            }
        },
    );
}

/// Show a provider row (and its group) once it has saved credentials to manage.
fn reveal(row: &adw::ActionRow, has_credentials: bool) {
    row.set_visible(has_credentials);
    if has_credentials && let Some(group) = row.ancestor(adw::PreferencesGroup::static_type()) {
        group.set_visible(true);
    }
}
