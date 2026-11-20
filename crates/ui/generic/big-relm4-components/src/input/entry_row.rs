// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Entry rows a screen reader names once.

use adw::prelude::*;
use relm4::gtk;

/// Build `builder`'s row, named by its title as [`name_by_title`] does.
#[must_use]
pub fn build(builder: adw::builders::EntryRowBuilder) -> adw::EntryRow {
    #[expect(
        clippy::disallowed_methods,
        reason = "the owner that names the row once"
    )]
    let row = builder.build();
    name_by_title(&row);
    row
}

/// Name `row` by its title, now and whenever the title changes. libadwaita 1.9
/// labels an entry row by both of its title labels, so a screen reader read
/// the title twice; a password row needs this after `password_row`.
pub fn name_by_title(row: &impl IsA<adw::EntryRow>) {
    let row = row.upcast_ref::<adw::EntryRow>();
    row.reset_relation(gtk::AccessibleRelation::LabelledBy);
    row.update_property(&[gtk::accessible::Property::Label(&row.title())]);
    row.connect_title_notify(|row| {
        row.update_property(&[gtk::accessible::Property::Label(&row.title())]);
    });
}
