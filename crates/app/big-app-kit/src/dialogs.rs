// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Dialog workflow contracts.
//!
//! The display-free specs (`BigDialogSpec`, `BigEntryValidation`, …) live
//! in `big_app_kit_core::dialogs` and are re-exported below; the libadwaita
//! `AlertDialog` builders stay here where gtk-rs is available.

use adw::prelude::*;
use relm4::gtk;

#[doc(inline)]
pub use big_app_kit_core::dialogs::{
    BigDialogKind, BigDialogResolved, BigDialogSpec, BigEntryValidation, BigEntryValidationTone,
    BigValidatedEntryDialogSpec, persist_dialog_size,
};

/// Build a single-button `AdwAlertDialog` with an empty content box.
/// Caller fills the returned `gtk::Box` and presents the dialog.
pub fn content_dialog(heading: &str, close_label: &str) -> (adw::AlertDialog, gtk::Box) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .close_response("close")
        .build();
    dialog.update_property(&[gtk::accessible::Property::Label(heading)]);
    dialog.add_response("close", close_label);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(16);
    content.set_margin_end(16);
    dialog.set_extra_child(Some(&content));
    (dialog, content)
}

/// Build a two-button dialog (cancel + custom action) with an empty
/// content box. `destructive = true` styles the action button in red.
pub fn content_action_dialog(
    heading: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    destructive: bool,
) -> (adw::AlertDialog, gtk::Box) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .close_response("cancel")
        .default_response(confirm_id)
        .build();
    dialog.update_property(&[gtk::accessible::Property::Label(heading)]);
    dialog.add_response("cancel", cancel_label);
    dialog.add_response(confirm_id, confirm_label);
    dialog.set_response_appearance(
        confirm_id,
        if destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(16);
    content.set_margin_end(16);
    dialog.set_extra_child(Some(&content));
    (dialog, content)
}

/// Build a one-button error dialog with the close button as both
/// default and close response.
pub fn error_dialog(heading: &str, body: &str, close_label: &str) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .close_response("close")
        .default_response("close")
        .build();
    dialog.update_property(&[gtk::accessible::Property::Label(heading)]);
    dialog.add_response("close", close_label);
    dialog
}

/// Build a confirm dialog (cancel + confirm). Cancel response id is
/// hard-coded to `"cancel"`; use [`confirm_dialog_with_cancel_id`] to
/// override.
pub fn confirm_dialog(
    heading: &str,
    body: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    destructive: bool,
) -> adw::AlertDialog {
    confirm_dialog_with_cancel_id(
        heading,
        body,
        "cancel",
        cancel_label,
        confirm_id,
        confirm_label,
        destructive,
    )
}

/// Like [`confirm_dialog`] but lets the caller pick the cancel
/// response id. Useful when the host code needs to disambiguate
/// cancel from a different "no-op" outcome.
pub fn confirm_dialog_with_cancel_id(
    heading: &str,
    body: &str,
    cancel_id: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    destructive: bool,
) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .close_response(cancel_id)
        .default_response(cancel_id)
        .build();
    dialog.add_response(cancel_id, cancel_label);
    dialog.add_response(confirm_id, confirm_label);
    dialog.set_response_appearance(
        confirm_id,
        if destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    dialog
}

/// Build a single-entry input dialog with cancel + suggested confirm
/// buttons and an accessible label on the entry.
pub fn entry_dialog(
    heading: &str,
    body: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    placeholder: &str,
    accessible_label: &str,
) -> (adw::AlertDialog, gtk::Entry) {
    entry_choice_dialog(
        heading,
        body,
        cancel_label,
        confirm_id,
        placeholder,
        accessible_label,
        &[(
            confirm_id,
            confirm_label,
            adw::ResponseAppearance::Suggested,
        )],
    )
}

/// Build an entry dialog with multiple custom response buttons. Each
/// `(id, label, appearance)` triple becomes a button; `default_response`
/// receives initial focus.
pub fn entry_choice_dialog(
    heading: &str,
    body: &str,
    cancel_label: &str,
    default_response: &str,
    placeholder: &str,
    accessible_label: &str,
    responses: &[(&str, &str, adw::ResponseAppearance)],
) -> (adw::AlertDialog, gtk::Entry) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .close_response("cancel")
        .default_response(default_response)
        .build();
    dialog.add_response("cancel", cancel_label);
    for (response_id, label, appearance) in responses {
        dialog.add_response(response_id, label);
        dialog.set_response_appearance(response_id, *appearance);
    }

    let entry = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .hexpand(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    dialog.set_extra_child(Some(&entry));

    (dialog, entry)
}

/// Like [`entry_dialog`] but pre-populates the entry with `initial_text`
/// and selects the whole region so the user can retype or tweak in place.
pub fn entry_dialog_with_initial(
    heading: &str,
    body: &str,
    cancel_label: &str,
    confirm_id: &str,
    confirm_label: &str,
    placeholder: &str,
    initial_text: &str,
    accessible_label: &str,
) -> (adw::AlertDialog, gtk::Entry) {
    let (dialog, entry) = entry_dialog(
        heading,
        body,
        cancel_label,
        confirm_id,
        confirm_label,
        placeholder,
        accessible_label,
    );
    entry.set_text(initial_text);
    entry.select_region(0, -1);
    (dialog, entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires live GTK display; keep rendered GTK smoke out of ordinary unit gate"]
    fn entry_dialog_with_initial_populates_and_selects() {
        if gtk::init().is_err() {
            eprintln!("skip: no display");
            return;
        }
        let (_dialog, entry) = entry_dialog_with_initial(
            "Rename",
            "Pick a new name",
            "Cancel",
            "rename",
            "Rename",
            "New name",
            "photo.jpg",
            "New file name",
        );
        assert_eq!(entry.text().as_str(), "photo.jpg");
        assert!(entry.selection_bounds().is_some());
    }
}
