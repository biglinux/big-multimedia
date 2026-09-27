// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Plain context popover with flat action rows.

use relm4::gtk;
use relm4::gtk::gdk;
use relm4::gtk::prelude::*;

use crate::text::escape_markup_text;

/// Right-click / long-press popover that hosts a vertical stack of
/// flat action buttons. Cheap to build, self-unparenting on close.
#[derive(Debug, Clone)]
pub struct BigContextPopover {
    popover: gtk::Popover,
    content: gtk::Box,
}

impl BigContextPopover {
    /// Creates a new instance.
    #[must_use]
    pub fn new(has_arrow: bool) -> Self {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .build();

        let popover = gtk::Popover::builder()
            .child(&content)
            .has_arrow(has_arrow)
            .autohide(true)
            .build();

        Self { popover, content }
    }

    /// Append a horizontal separator between two groups of buttons.
    pub fn append_separator(&self) {
        self.content
            .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }

    /// Append an action button. The popover popdowns before
    /// `on_clicked` fires so handlers can launch dialogs without the
    /// popover stealing back focus.
    pub fn append_button<F>(&self, label: impl AsRef<str>, css_classes: &[&str], on_clicked: F)
    where
        F: Fn() + 'static,
    {
        let label = label.as_ref();
        let row_label_markup = escape_markup_text(label);
        let row_label = gtk::Label::builder()
            .label(format!("<span weight=\"normal\">{row_label_markup}</span>"))
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .single_line_mode(true)
            .use_markup(true)
            .xalign(0.0)
            .build();
        let button = gtk::Button::builder()
            .child(&row_label)
            .hexpand(true)
            .build();
        button.update_property(&[gtk::accessible::Property::Label(label)]);
        button.add_css_class("flat");
        for css_class in css_classes {
            button.add_css_class(css_class);
        }

        let popover = self.popover.downgrade();
        button.connect_clicked(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            on_clicked();
        });
        self.content.append(&button);
    }

    /// Attach the popover to `parent` and pop it up pointing at the
    /// `(x, y)` coordinate inside the parent.
    pub fn popup_at(self, parent: &impl IsA<gtk::Widget>, x: f64, y: f64) {
        self.popover.set_parent(parent.as_ref());
        self.popover
            .set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));

        self.popup_with_auto_unparent();
    }

    fn popup_with_auto_unparent(self) {
        auto_unparent_on_close(&self.popover);
        self.popover.popup();
    }
}

/// Make a custom-parented popover clean itself up when closed.
///
/// GTK4 does NOT unparent a popover attached via `set_parent` when its parent
/// widget is disposed: the parent finalizes "with children left" and the live
/// popover pins its whole child subtree (measured leak class — a per-click
/// context menu accumulates one popover per use, forever). Call this right
/// after `set_parent` on any transient popover; popovers owned by a struct
/// should instead unparent in that owner's `Drop`.
pub fn auto_unparent_on_close(popover: &impl IsA<gtk::Popover>) {
    // Actions finish after `closed`; preserve their ancestry until the idle.
    // Keep the parent alive too, in case its owner closes during that action.
    popover.as_ref().connect_closed(move |popover| {
        let parent = popover.parent();
        let popover = popover.clone();
        gtk::glib::idle_add_local_once(move || {
            if popover.parent().is_some() {
                popover.unparent();
            }
            drop(parent);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    #[ignore = "requires an isolated GTK session"]
    fn close_keeps_the_action_context_until_idle_and_finalizes() {
        gtk::init().unwrap();
        let context = gtk::glib::MainContext::default();
        for _ in 0..25 {
            let parent = gtk::Button::new();
            let popover = gtk::Popover::new();
            popover.set_parent(&parent);
            let finalized = Rc::new(Cell::new(false));
            struct Finalized(Rc<Cell<bool>>);
            impl Drop for Finalized {
                fn drop(&mut self) {
                    self.0.set(true);
                }
            }
            // SAFETY: this test owns the key and never reads the stored data.
            unsafe {
                popover.set_qdata(
                    gtk::glib::Quark::from_str("popover-finalized"),
                    Finalized(finalized.clone()),
                );
            }
            auto_unparent_on_close(&popover);
            popover.emit_by_name::<()>("closed", &[]);
            assert!(
                popover.parent().is_some(),
                "the activated action still needs its parent"
            );
            drop(parent);
            drop(popover);
            while context.pending() {
                context.iteration(false);
            }
            assert!(finalized.get(), "popover did not finalize after idle");
        }
    }
}
