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

use adw::prelude::*;
use big_relm4_components::feedback::tooltip;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
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
use relm4::ComponentSender;

use crate::i18n::i18n;
use crate::media::iptv::{IptvKind, IptvProvider, IptvProviderStore};
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
        BigBoundedListBox::new(BigBoundedListBoxSpec::new(24).css_class("boxed-list"));
    let list = provider_list.root().clone();
    page.content().append(&list);
    let status_store = store.clone();
    let retry_store = store.clone();
    page.content()
        .append(&crate::settings::saving::document_status(
            move || status_store.borrow().save_status(),
            move || retry_store.borrow().retry_save(),
        ));

    let dialog = adw::Dialog::builder()
        .title(i18n("IPTV Providers"))
        .content_width(600)
        .content_height(480)
        .child(&page.into_root())
        .build();

    let dialog_weak = dialog.downgrade();
    let store_for_add = store.clone();
    let list_weak = list.downgrade();
    add_provider_button.connect_clicked(move |_| {
        let Some(parent) = dialog_weak.upgrade() else {
            return;
        };
        let list = list_weak.clone();
        let store = store_for_add.clone();
        show_provider_form(Some(&parent), IptvProvider::default(), move |p| {
            store.borrow_mut().add(p);
            if let Some(list) = list.upgrade() {
                rebuild_list(&list, &store);
            }
        });
    });

    rebuild_list(&list, &store);

    let sender = sender.clone();
    dialog.connect_closed(move |_| {
        sender.input(VideoIptvBrowserInput::ProvidersChanged);
    });
    dialog.present(Some(parent));
}

fn rebuild_list(list: &gtk::ListBox, store: &Rc<RefCell<IptvProviderStore>>) {
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
        let provider_row = BigEditableActionRow::new(
            BigEditableActionRowSpec::new(&provider.name, i18n("Edit"), i18n("Remove"))
                .subtitle(provider.source.summary())
                .prefix_icon_name(source_icon(&provider.source)),
        );
        let (row, edit_btn, remove_btn) = provider_row.into_parts();

        let list_weak = list.downgrade();
        let store_for_edit = store.clone();
        edit_btn.connect_clicked(move |btn| {
            let parent = btn
                .ancestor(adw::Dialog::static_type())
                .and_downcast::<adw::Dialog>();
            let provider = store_for_edit
                .borrow()
                .providers
                .get(idx)
                .cloned()
                .unwrap_or_default();
            let list = list_weak.clone();
            let store = store_for_edit.clone();
            show_provider_form(parent.as_ref(), provider, move |p| {
                store.borrow_mut().update(idx, p);
                if let Some(list) = list.upgrade() {
                    rebuild_list(&list, &store);
                }
            });
        });

        let list_weak = list.downgrade();
        let store_for_remove = store.clone();
        remove_btn.connect_clicked(move |_| {
            store_for_remove.borrow_mut().remove(idx);
            if let Some(list) = list_weak.upgrade() {
                rebuild_list(&list, &store_for_remove);
            }
        });

        list.append(&row);
    }
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
    let form = adw::Dialog::builder()
        .title(i18n("IPTV Provider"))
        .content_width(540)
        .content_height(520)
        .build();

    let page = BigDialogPage::scrolled_clamped(520, 420);
    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Save")));
    let inputs = ProviderFormInputs::build(&initial);
    page.content().append(&inputs.content);
    page.root().add_bottom_bar(footer.root());
    form.set_child(Some(&page.into_root()));

    let form_weak = form.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(f) = form_weak.upgrade() {
            f.close();
        }
    });

    let form_weak = form.downgrade();
    let on_save = Rc::new(on_save);
    footer.accept_button().connect_clicked(move |_| {
        let Some(provider) = inputs.collect() else {
            return;
        };
        on_save(provider);
        if let Some(f) = form_weak.upgrade() {
            f.close();
        }
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
        let name_row = adw::EntryRow::builder().title(i18n("Name")).build();
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
        let url_row = adw::EntryRow::builder().title(i18n("URL")).build();
        url_group.add(&url_row);

        let path_group = BigPreferencesGroupSpec::untitled().build();
        let path_row = adw::EntryRow::builder().title(i18n("File path")).build();
        path_group.add(&path_row);

        let xtream_group = BigPreferencesGroupSpec::untitled().build();
        let server_row = adw::EntryRow::builder().title(i18n("Server URL")).build();
        let username_row = adw::EntryRow::builder().title(i18n("Username")).build();
        let password_row = big_gtk_kit::password::password_row(
            adw::PasswordEntryRow::builder().title(i18n("Password")),
        );
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

        content.append(&url_group);
        content.append(&path_group);
        content.append(&xtream_group);

        let headers_group = BigPreferencesGroupSpec::new(i18n("HTTP headers (optional)")).build();
        let ua_row = adw::EntryRow::builder().title(i18n("User-Agent")).build();
        ua_row.set_text(&initial.user_agent);
        let referer_row = adw::EntryRow::builder().title(i18n("Referer")).build();
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
    fn collect(&self) -> Option<IptvProvider> {
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
            source,
            user_agent: self.ua_row.text().trim().to_string(),
            referer: self.referer_row.text().trim().to_string(),
            order: 0,
        })
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
