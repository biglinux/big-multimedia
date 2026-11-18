//! "Manage IPTV providers" dialog.
//!
//! Lists every provider in `IptvProviderStore`, with Add / Edit / Remove
//! actions. Editing opens a form dialog that toggles between M3U URL, local M3U
//! file, and Xtream Codes inputs. On dialog close the
//! [`VideoIptvBrowser`] component reloads the store and
//! refreshes its provider dropdown.
//!
//! Video-local helper presented by the browser component; signals "closed"
//! through a relm4 [`ComponentSender`].

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

use adw::prelude::*;
use big_os_kit::keyring::{SecretService, Vault};
use big_relm4_components::feedback::tooltip;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::input::entry_row;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};
use big_relm4_components::list::bounded_list_box::{
    BigBoundedListBox, BigBoundedListBoxSpec, clear_list_box_rows,
};
use big_relm4_components::list::detail_row::{BigDetailRow, BigDetailRowSpec};
use big_relm4_components::list::editable_action_row::{
    BigEditableActionRow, BigEditableActionRowSpec,
};
use gtk::glib;
use relm4::ComponentSender;

use crate::i18n::i18n;
use crate::media::iptv::{IptvKind, IptvProvider, IptvProviderStore, SECRET_SCOPE};
use crate::video::ui::iptv::{VideoIptvBrowser, VideoIptvBrowserInput};

/// Present the providers-management dialog over `parent`, notifying the browser
/// component (via `sender`) when it closes so the dropdown can refresh.
pub fn present_providers_dialog(
    parent: &impl IsA<gtk::Widget>,
    store: Rc<RefCell<IptvProviderStore>>,
    sender: ComponentSender<VideoIptvBrowser>,
) {
    let page = BigDialogPage::scrolled_clamped(560, 480);
    let add_provider_button = tooltip::icon_button("list-add-symbolic", &i18n("Add provider"), &[]);
    page.header().pack_end(&add_provider_button);

    let provider_list =
        BigBoundedListBox::new(BigBoundedListBoxSpec::default().css_class("boxed-list"));
    let list = provider_list.root().clone();
    page.content().append(&list);
    let keyring_note = gtk::Label::builder()
        .label(i18n(
            "A replaced or removed password could not be deleted from your keyring. You can delete the \u{201C}Big Multimedia IPTV provider\u{201D} item it left with your keyring manager.",
        ))
        .wrap(true)
        .selectable(true)
        .xalign(0.0)
        .css_classes(["caption", "warning"])
        .visible(false)
        .build();
    page.content().append(&keyring_note);
    let status_store = store.clone();
    let retry_store = store.clone();
    page.content()
        .append(&crate::settings::saving::document_status(
            move || status_store.borrow().save_status(),
            move || retry_store.borrow().retry_save(),
        ));

    let dialog = big_gtk_kit::dialog::Dialog::builder()
        .title(i18n("IPTV Providers"))
        .content_width(600)
        .content_height(480)
        .child(&page.into_root())
        .build();

    let dialog_weak = dialog.downgrade();
    let store_for_add = store.clone();
    let list_weak = list.downgrade();
    let note_weak = keyring_note.downgrade();
    add_provider_button.connect_clicked(move |_| {
        let Some(parent) = dialog_weak.upgrade() else {
            return;
        };
        let list = list_weak.clone();
        let note = note_weak.clone();
        let store = store_for_add.clone();
        show_provider_form(Some(parent.dialog()), IptvProvider::default(), move |p| {
            store.borrow_mut().add(p);
            if let (Some(list), Some(note)) = (list.upgrade(), note.upgrade()) {
                rebuild_list(&list, &store, &note);
            }
        });
    });

    rebuild_list(&list, &store, &keyring_note);

    let sender = sender.clone();
    dialog.connect_closed(move |_| {
        sender.input(VideoIptvBrowserInput::ProvidersChanged);
    });
    dialog.present(Some(parent));
}

fn rebuild_list(list: &gtk::ListBox, store: &Rc<RefCell<IptvProviderStore>>, note: &gtk::Label) {
    clear_list_box_rows(list);

    let providers = store.borrow().providers.clone();
    if providers.is_empty() {
        let row = BigDetailRow::new(
            BigDetailRowSpec::new(i18n("No providers yet"))
                .subtitle(i18n("Add an IPTV provider to get started")),
        )
        .into_root();
        list.append(&row);
        return;
    }

    for (idx, provider) in providers.into_iter().enumerate() {
        let mut subtitle = provider.source.summary();
        if provider.plaintext_secret().is_some() {
            subtitle.push('\n');
            subtitle.push_str(&i18n(
                "Password not in your keyring yet. Unlock your keyring, then close this window to try again.",
            ));
        }
        let provider_row = BigEditableActionRow::new(
            BigEditableActionRowSpec::new(&provider.name, i18n("Edit"), i18n("Remove"))
                .subtitle(subtitle)
                .prefix_icon_name(source_icon(&provider.source)),
        );
        let (row, edit_btn, remove_btn) = provider_row.into_parts();

        let list_weak = list.downgrade();
        let note_weak = note.downgrade();
        let store_for_edit = store.clone();
        edit_btn.connect_clicked(move |btn| {
            edit_provider(btn, idx, &store_for_edit, &list_weak, &note_weak);
        });

        let list_weak = list.downgrade();
        let note_weak = note.downgrade();
        let store_for_remove = store.clone();
        remove_btn.connect_clicked(move |_| {
            remove_provider(idx, &store_for_remove, &list_weak, &note_weak);
        });

        list.append(&row);
    }
}

/// Open the form on the provider at `index`; saving replaces it and deletes
/// a keyring item the edit replaced.
fn edit_provider(
    button: &gtk::Button,
    index: usize,
    store: &Rc<RefCell<IptvProviderStore>>,
    list: &glib::WeakRef<gtk::ListBox>,
    note: &glib::WeakRef<gtk::Label>,
) {
    let parent = button
        .ancestor(adw::Dialog::static_type())
        .and_downcast::<adw::Dialog>();
    let provider = store
        .borrow()
        .providers
        .get(index)
        .cloned()
        .unwrap_or_default();
    let replaced_secret = provider.secret_id.clone();
    let list = list.clone();
    let note = note.clone();
    let store = store.clone();
    show_provider_form(parent.as_ref(), provider, move |p| {
        let replaced = p.secret_id != replaced_secret;
        store.borrow_mut().update(index, p);
        if replaced {
            forget_secret(replaced_secret.clone(), note.clone());
        }
        if let (Some(list), Some(note)) = (list.upgrade(), note.upgrade()) {
            rebuild_list(&list, &store, &note);
        }
    });
}

/// Remove the provider at `index` and the keyring item it named.
fn remove_provider(
    index: usize,
    store: &Rc<RefCell<IptvProviderStore>>,
    list: &glib::WeakRef<gtk::ListBox>,
    note: &glib::WeakRef<gtk::Label>,
) {
    let removed_secret = store
        .borrow()
        .providers
        .get(index)
        .map(|provider| provider.secret_id.clone());
    store.borrow_mut().remove(index);
    if let Some(secret_id) = removed_secret {
        forget_secret(secret_id, note.clone());
    }
    if let (Some(list), Some(note)) = (list.upgrade(), note.upgrade()) {
        rebuild_list(&list, store, &note);
    }
}

/// Delete the keyring item `secret_id` no provider names any more, and show
/// `note` when the keyring keeps it.
fn forget_secret(secret_id: String, note: glib::WeakRef<gtk::Label>) {
    if secret_id.is_empty() {
        return;
    }
    crate::settings::saving::acknowledged_task(
        true,
        move || {
            let cancelled = AtomicBool::new(false);
            SecretService {
                cancelled: &cancelled,
            }
            .clear(SECRET_SCOPE, &secret_id)
        },
        move |result| {
            if result.is_err()
                && let Some(note) = note.upgrade()
            {
                note.set_visible(true);
            }
        },
    );
}

fn source_icon(source: &IptvKind) -> &'static str {
    match source {
        IptvKind::M3uUrl { .. } => "network-transmit-receive-symbolic",
        IptvKind::M3uLocal { .. } => "folder-documents-symbolic",
        IptvKind::Xtream { .. } => "media-playback-start-symbolic",
    }
}

fn show_provider_form<F>(parent: Option<&adw::Dialog>, initial: IptvProvider, on_save: F)
where
    F: Fn(IptvProvider) + 'static,
{
    let form = big_gtk_kit::dialog::Dialog::builder()
        .title(i18n("IPTV Provider"))
        .content_width(540)
        .content_height(520)
        .build();

    let page = BigDialogPage::scrolled_clamped(520, 420);
    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Save")));
    let inputs = ProviderFormInputs::build(&initial);
    page.content().append(&inputs.content);
    let keyring_status = gtk::Label::builder()
        .wrap(true)
        .selectable(true)
        .xalign(0.0)
        .css_classes(["caption"])
        .visible(false)
        .build();
    page.content().append(&keyring_status);
    page.root().add_bottom_bar(footer.root());
    form.set_child(Some(&page.into_root()));

    let form_weak = form.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(f) = form_weak.upgrade() {
            f.close();
        }
    });

    let form_weak = form.downgrade();
    let status_weak = keyring_status.downgrade();
    let on_save = Rc::new(on_save);
    footer.accept_button().connect_clicked(move |button| {
        let Some(mut provider) = inputs.collect(&initial) else {
            return;
        };
        if provider.plaintext_secret().is_none() {
            on_save(provider);
            if let Some(f) = form_weak.upgrade() {
                f.close();
            }
            return;
        }
        button.set_sensitive(false);
        if let Some(status) = status_weak.upgrade() {
            status.remove_css_class("error");
            status.set_label(&i18n("Saving the password in your keyring\u{2026}"));
            status.set_visible(true);
        }
        let button = button.downgrade();
        let status = status_weak.clone();
        let form = form_weak.clone();
        let on_save = on_save.clone();
        crate::settings::saving::acknowledged_task(
            true,
            move || {
                let cancelled = AtomicBool::new(false);
                provider
                    .protect_secret(&SecretService {
                        cancelled: &cancelled,
                    })
                    .map(|()| provider)
            },
            move |result| {
                if let Ok(provider) = result {
                    on_save(provider);
                    if let Some(f) = form.upgrade() {
                        f.close();
                    }
                    return;
                }
                if let Some(status) = status.upgrade() {
                    status.add_css_class("error");
                    status.set_label(&i18n(
                        "Could not save the password in your keyring, so nothing was saved. Unlock your keyring, then save again.",
                    ));
                }
                if let Some(button) = button.upgrade() {
                    button.set_sensitive(true);
                }
            },
        );
    });

    if let Some(parent) = parent {
        form.present(Some(parent));
    } else {
        form.present(None::<&adw::Dialog>);
    }
}

/// Field widgets of the provider form, kept together so the save handler can
/// read them and the type combo can toggle the relevant group's visibility.
struct ProviderFormInputs {
    content: gtk::Box,
    name_row: adw::EntryRow,
    type_row: gtk::DropDown,
    url_row: adw::EntryRow,
    path_row: adw::EntryRow,
    server_row: adw::EntryRow,
    username_row: adw::EntryRow,
    password_row: adw::PasswordEntryRow,
    ua_row: adw::EntryRow,
    referer_row: adw::EntryRow,
}

impl ProviderFormInputs {
    fn build(initial: &IptvProvider) -> Self {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();

        let name_group = BigPreferencesGroupSpec::untitled().build();
        let name_row = entry_row::build(adw::EntryRow::builder().title(i18n("Name")));
        name_row.set_text(&initial.name);
        name_group.add(&name_row);
        content.append(&name_group);

        let type_group = BigPreferencesGroupSpec::untitled().build();
        let initial_index = source_index(&initial.source);
        let (type_row_widget, type_row) = BigDropdownRow::new(
            BigDropdownRowSpec::new(
                i18n("Type"),
                [
                    i18n("M3U URL"),
                    i18n("Local M3U file"),
                    i18n("Xtream Codes"),
                ],
            )
            .selected(initial_index),
        )
        .into_parts();
        type_group.add(&type_row_widget);
        content.append(&type_group);

        let url_group = BigPreferencesGroupSpec::untitled().build();
        let url_row = entry_row::build(adw::EntryRow::builder().title(i18n("URL")));
        url_group.add(&url_row);

        let path_group = BigPreferencesGroupSpec::untitled().build();
        let path_row = entry_row::build(adw::EntryRow::builder().title(i18n("File path")));
        path_group.add(&path_row);

        let xtream_group = BigPreferencesGroupSpec::untitled().build();
        let server_row = entry_row::build(adw::EntryRow::builder().title(i18n("Server URL")));
        let username_row = entry_row::build(adw::EntryRow::builder().title(i18n("Username")));
        let password_row = big_gtk_kit::password::password_row(
            adw::PasswordEntryRow::builder().title(i18n("Password")),
        );
        entry_row::name_by_title(&password_row);
        xtream_group.add(&server_row);
        xtream_group.add(&username_row);
        xtream_group.add(&password_row);

        match &initial.source {
            IptvKind::M3uUrl { url } => url_row.set_text(url),
            IptvKind::M3uLocal { path } => path_row.set_text(path),
            IptvKind::Xtream {
                server,
                username,
                password,
            } => {
                server_row.set_text(server);
                username_row.set_text(username);
                password_row.set_text(password);
            }
        }
        if !initial.secret_id.is_empty() {
            url_group.set_description(Some(&i18n(
                "The full address is saved in your keyring. Leave it as shown to keep it.",
            )));
            xtream_group.set_description(Some(&i18n(
                "The password is saved in your keyring. Leave Password empty to keep it.",
            )));
        }

        content.append(&url_group);
        content.append(&path_group);
        content.append(&xtream_group);

        let headers_group = BigPreferencesGroupSpec::new(i18n("HTTP headers (optional)")).build();
        let ua_row = entry_row::build(adw::EntryRow::builder().title(i18n("User-Agent")));
        ua_row.set_text(&initial.user_agent);
        let referer_row = entry_row::build(adw::EntryRow::builder().title(i18n("Referer")));
        referer_row.set_text(&initial.referer);
        headers_group.add(&ua_row);
        headers_group.add(&referer_row);
        content.append(&headers_group);

        wire_type_visibility(
            &type_row,
            &url_group,
            &path_group,
            &xtream_group,
            initial_index,
        );

        Self {
            content,
            name_row,
            type_row,
            url_row,
            path_row,
            server_row,
            username_row,
            password_row,
            ua_row,
            referer_row,
        }
    }

    /// Read the form into a provider, or `None` if the (required) name is blank.
    /// A typed password or address stays in plain text until saved.
    fn collect(&self, initial: &IptvProvider) -> Option<IptvProvider> {
        let name = self.name_row.text().trim().to_string();
        if name.is_empty() {
            return None;
        }
        let source = match self.type_row.selected() {
            0 => IptvKind::M3uUrl {
                url: self.url_row.text().trim().to_string(),
            },
            1 => IptvKind::M3uLocal {
                path: self.path_row.text().trim().to_string(),
            },
            _ => IptvKind::Xtream {
                server: self.server_row.text().trim().to_string(),
                username: self.username_row.text().trim().to_string(),
                password: self.password_row.text().to_string(),
            },
        };
        Some(IptvProvider {
            name,
            secret_id: kept_secret(initial, &source),
            source,
            user_agent: self.ua_row.text().trim().to_string(),
            referer: self.referer_row.text().trim().to_string(),
            order: 0,
        })
    }
}

/// The keyring item `initial` names, while the form left its secret alone: the
/// redacted address as shown, or an empty Xtream password.
fn kept_secret(initial: &IptvProvider, source: &IptvKind) -> String {
    let kept = match (&initial.source, source) {
        (IptvKind::M3uUrl { url: before }, IptvKind::M3uUrl { url }) => url == before,
        (IptvKind::Xtream { .. }, IptvKind::Xtream { password, .. }) => password.is_empty(),
        _ => false,
    };
    if kept {
        initial.secret_id.clone()
    } else {
        String::new()
    }
}

/// Combo index for a source kind (URL = 0, local = 1, Xtream = 2).
fn source_index(source: &IptvKind) -> u32 {
    match source {
        IptvKind::M3uUrl { .. } => 0,
        IptvKind::M3uLocal { .. } => 1,
        IptvKind::Xtream { .. } => 2,
    }
}

/// Show only the input group matching the selected source type; re-sync on
/// every combo change.
fn wire_type_visibility(
    type_row: &gtk::DropDown,
    url_group: &adw::PreferencesGroup,
    path_group: &adw::PreferencesGroup,
    xtream_group: &adw::PreferencesGroup,
    initial_index: u32,
) {
    let url_weak = url_group.downgrade();
    let path_weak = path_group.downgrade();
    let xtream_weak = xtream_group.downgrade();
    let sync = move |selected: u32| {
        if let Some(g) = url_weak.upgrade() {
            g.set_visible(selected == 0);
        }
        if let Some(g) = path_weak.upgrade() {
            g.set_visible(selected == 1);
        }
        if let Some(g) = xtream_weak.upgrade() {
            g.set_visible(selected == 2);
        }
    };
    sync(initial_index);
    type_row.connect_selected_notify(move |row| sync(row.selected()));
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAVED: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn saved(source: IptvKind) -> IptvProvider {
        IptvProvider {
            source,
            secret_id: SAVED.into(),
            ..IptvProvider::default()
        }
    }

    fn xtream(password: &str) -> IptvKind {
        IptvKind::Xtream {
            server: "http://panel".into(),
            username: "ana".into(),
            password: password.into(),
        }
    }

    #[test]
    fn untouched_secrets_stay_in_the_keyring() {
        let shown = "https://h/get.php?username=***&password=***";
        let list = saved(IptvKind::M3uUrl { url: shown.into() });
        assert_eq!(
            kept_secret(&list, &IptvKind::M3uUrl { url: shown.into() }),
            SAVED
        );
        assert_eq!(kept_secret(&saved(xtream("")), &xtream("")), SAVED);
    }

    #[test]
    fn edited_secrets_drop_the_saved_item() {
        let list = saved(IptvKind::M3uUrl {
            url: "https://h/get.php?password=***".into(),
        });
        let edited = IptvKind::M3uUrl {
            url: "https://h/get.php?password=new".into(),
        };
        assert_eq!(kept_secret(&list, &edited), "");
        assert_eq!(kept_secret(&saved(xtream("")), &xtream("new")), "");
        assert_eq!(kept_secret(&saved(xtream("")), &edited), "");
        assert_eq!(kept_secret(&IptvProvider::default(), &xtream("")), "");
    }
}
