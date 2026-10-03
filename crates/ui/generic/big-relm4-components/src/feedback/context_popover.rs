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

    /// Append a row named `label` of icon buttons, one per `(icon, accessible
    /// name)` choice. Pressing one pops the menu down and calls `on_choice`
    /// with its index.
    pub fn append_icon_choices<F>(&self, label: &str, choices: &[(&str, String)], on_choice: F)
    where
        F: Fn(usize) + 'static,
    {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .build();
        let name = gtk::Label::builder()
            .label(label)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .xalign(0.0)
            .margin_start(10)
            .margin_end(10)
            .build();
        name.add_css_class("dim-label");
        row.append(&name);
        let on_choice = std::rc::Rc::new(on_choice);
        for (index, (icon, accessible)) in choices.iter().enumerate() {
            let button = gtk::Button::builder()
                .icon_name(*icon)
                .tooltip_text(accessible.as_str())
                .build();
            button.add_css_class("flat");
            button.update_property(&[gtk::accessible::Property::Label(accessible)]);
            let popover = self.popover.downgrade();
            let on_choice = std::rc::Rc::clone(&on_choice);
            button.connect_clicked(move |_| {
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
                on_choice(index);
            });
            row.append(&button);
        }
        row.update_property(&[gtk::accessible::Property::Label(label)]);
        self.content.append(&row);
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
        big_gtk_kit::popover::auto_unparent_on_close(&self.popover);
        self.popover.popup();
    }
}
