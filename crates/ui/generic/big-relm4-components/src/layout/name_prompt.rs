// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Dialog that asks for one name: an entry row, Cancel and an accept button
//! enabled while the entry holds a valid name.

use std::rc::Rc;

use adw::prelude::*;
use relm4::gtk;

use super::dialog_footer::{BigDialogFooter, BigDialogFooterSpec};
use super::dialog_page::{BigDialogPage, focus_when_shown};
use super::preferences::BigPreferencesGroupSpec;

/// What a [`prompt_name`] dialog shows; every string is already translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigNamePromptSpec {
    /// Dialog title.
    pub title: String,
    /// Dim text above the entry, when there is one.
    pub description: Option<String>,
    /// Entry row title.
    pub entry_title: String,
    /// Text the entry starts with, selected when the dialog shows.
    pub prefill: String,
    /// Accept button label.
    pub accept_label: String,
    /// Dialog content width in pixels.
    pub content_width: i32,
    /// Dialog content height in pixels.
    pub content_height: i32,
}

/// Present a dialog asking for one name over `parent`.
///
/// The accept button and the entry's apply button act only while `validate`
/// returns a name; `on_accept` receives that name and returns whether the
/// dialog closes. Cancel closes it without calling `on_accept`.
pub fn prompt_name(
    parent: Option<&gtk::Widget>,
    spec: &BigNamePromptSpec,
    validate: impl Fn(&str) -> Option<String> + 'static,
    on_accept: impl Fn(String) -> bool + 'static,
) {
    prompt_name_with_reply(parent, spec, validate, move |name, reply| {
        if on_accept(name) {
            reply.close();
        }
    });
}

/// [`prompt_name`] for an outcome that arrives later, such as work on a
/// worker thread: `on_accept` receives the name and a [`BigNamePromptReply`].
/// The entry and the accept button stay insensitive until the reply closes
/// the dialog or is dropped, which keeps it open for another try.
pub fn prompt_name_with_reply(
    parent: Option<&gtk::Widget>,
    spec: &BigNamePromptSpec,
    validate: impl Fn(&str) -> Option<String> + 'static,
    on_accept: impl Fn(String, BigNamePromptReply) + 'static,
) {
    let dialog = big_gtk_kit::dialog::Dialog::builder()
        .title(spec.title.as_str())
        .content_width(spec.content_width)
        .content_height(spec.content_height)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);
    if let Some(description) = &spec.description {
        page.append_description(description);
    }

    let group = BigPreferencesGroupSpec::untitled().build();
    let entry = adw::EntryRow::builder()
        .title(spec.entry_title.as_str())
        .text(spec.prefill.as_str())
        .show_apply_button(true)
        .build();
    group.add(&entry);
    page.content().append(&group);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(
        crate::i18n::t("Cancel"),
        spec.accept_label.as_str(),
    ));
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));
    connect_handlers(&dialog, &entry, &footer, validate, on_accept);
    focus_when_shown(&entry, 0, -1);
    dialog.present(parent);
}

/// The outcome of one accepted name, owed to the prompt it came from.
pub struct BigNamePromptReply {
    dialog: big_gtk_kit::dialog::WeakDialog,
    entry: gtk::glib::WeakRef<adw::EntryRow>,
    accept_button: gtk::glib::WeakRef<gtk::Button>,
    closed: bool,
}

impl BigNamePromptReply {
    /// Close the prompt, unless it was cancelled meanwhile.
    pub fn close(mut self) {
        self.closed = self.dialog.upgrade().is_some_and(|dialog| dialog.close());
    }
}

impl Drop for BigNamePromptReply {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        if let Some(accept_button) = self.accept_button.upgrade() {
            accept_button.set_sensitive(true);
        }
        if let Some(entry) = self.entry.upgrade() {
            entry.set_sensitive(true);
            entry.grab_focus();
        }
    }
}

/// The accept handler owns the entry, so the entry's handlers hold the button weakly.
fn connect_handlers(
    dialog: &big_gtk_kit::dialog::Dialog,
    entry: &adw::EntryRow,
    footer: &BigDialogFooter,
    validate: impl Fn(&str) -> Option<String> + 'static,
    on_accept: impl Fn(String, BigNamePromptReply) + 'static,
) {
    let validate = Rc::new(validate);
    footer
        .accept_button()
        .set_sensitive(validate(&entry.text()).is_some());
    let accept_button = footer.accept_button().downgrade();
    entry.connect_changed({
        let validate = Rc::clone(&validate);
        move |row| {
            if let Some(accept_button) = accept_button.upgrade() {
                accept_button.set_sensitive(validate(&row.text()).is_some());
            }
        }
    });
    let accept_button = footer.accept_button().downgrade();
    entry.connect_apply(move |_| {
        if let Some(accept_button) = accept_button.upgrade()
            && accept_button.is_sensitive()
        {
            accept_button.emit_clicked();
        }
    });

    let dialog_weak = dialog.downgrade();
    let entry_for_accept = entry.clone();
    footer
        .accept_button()
        .connect_clicked(move |accept_button| {
            let Some(name) = validate(&entry_for_accept.text()) else {
                return;
            };
            entry_for_accept.set_sensitive(false);
            accept_button.set_sensitive(false);
            on_accept(
                name,
                BigNamePromptReply {
                    dialog: dialog_weak.clone(),
                    entry: entry_for_accept.downgrade(),
                    accept_button: accept_button.downgrade(),
                    closed: false,
                },
            );
        });

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });
}

/// `text` without surrounding whitespace, or `None` when nothing else is left:
/// the validator for names that only need to be non-blank.
#[must_use]
pub fn trimmed_name(text: &str) -> Option<String> {
    let name = text.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn trimmed_name_rejects_blank_text() {
        assert_eq!(trimmed_name("  Road Trip \t"), Some("Road Trip".to_owned()));
        assert_eq!(trimmed_name(" \t\n"), None);
        assert_eq!(trimmed_name(""), None);
    }

    /// Counts its own drops: stored as qdata, it drops only when the object
    /// finalizes.
    struct Finalized(Rc<Cell<usize>>);

    impl Drop for Finalized {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    fn watch(object: &impl IsA<gtk::glib::Object>, count: &Rc<Cell<usize>>) {
        // SAFETY: this test owns the key and never reads the stored value.
        unsafe {
            object.set_qdata(
                gtk::glib::Quark::from_str("name-prompt-finalized"),
                Finalized(count.clone()),
            );
        }
    }

    fn pump() {
        let context = gtk::glib::MainContext::default();
        for _ in 0..50 {
            while context.iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    fn descendants(root: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut found = vec![root.clone()];
        let mut child = root.first_child();
        while let Some(widget) = child {
            found.extend(descendants(&widget));
            child = widget.next_sibling();
        }
        found
    }

    fn button_labeled(widgets: &[gtk::Widget], label: &str) -> gtk::Button {
        widgets
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::Button>())
            .find(|button| button.label().as_deref() == Some(label))
            .expect("footer button")
            .clone()
    }

    /// Accept through the entry's apply or press Cancel, then wait until the
    /// dialog has left `window`.
    fn close_shown_prompt(
        window: &adw::Window,
        accept: bool,
        entries: &Rc<Cell<usize>>,
        accept_buttons: &Rc<Cell<usize>>,
    ) {
        let dialog = window.visible_dialog().expect("prompt shown");
        let widgets = descendants(dialog.upcast_ref());
        let entry = widgets
            .iter()
            .find_map(|widget| widget.downcast_ref::<adw::EntryRow>())
            .expect("name entry")
            .clone();
        let accept_button = button_labeled(&widgets, "Save");
        watch(&entry, entries);
        watch(&accept_button, accept_buttons);
        if accept {
            entry.emit_by_name::<()>("apply", &[]);
        } else {
            button_labeled(&widgets, "Cancel").emit_clicked();
        }
        drop((widgets, entry, accept_button));
        while dialog.parent().is_some() {
            pump();
        }
        drop(dialog);
        pump();
    }

    #[test]
    #[ignore = "requires an isolated GTK session"]
    fn closed_prompts_finalize_their_entry_and_buttons() {
        gtk::init().expect("isolated GTK session");
        adw::init().expect("libadwaita");
        big_gtk_kit::install();
        let window = adw::Window::builder()
            .default_width(800)
            .default_height(600)
            .build();
        window.present();
        pump();
        let spec = BigNamePromptSpec {
            title: "Rename".into(),
            description: Some("Choose a new name.".into()),
            entry_title: "Name".into(),
            prefill: "Road Mix".into(),
            accept_label: "Save".into(),
            content_width: 420,
            content_height: 240,
        };
        let accepted = Rc::new(Cell::new(0));
        let entries = Rc::new(Cell::new(0));
        let accept_buttons = Rc::new(Cell::new(0));
        for accept in [true, false] {
            for _ in 0..25 {
                let calls = accepted.clone();
                prompt_name(
                    Some(window.upcast_ref()),
                    &spec,
                    trimmed_name,
                    move |name| {
                        assert_eq!(name, "Road Mix");
                        calls.set(calls.get() + 1);
                        true
                    },
                );
                pump();
                close_shown_prompt(&window, accept, &entries, &accept_buttons);
            }
        }
        window.destroy();
        drop(window);
        pump();
        assert_eq!(accepted.get(), 25, "accepted through the entry's apply");
        assert_eq!(entries.get(), 50, "a closed prompt retained its entry");
        assert_eq!(
            accept_buttons.get(),
            50,
            "a closed prompt retained its accept button"
        );
    }
}
