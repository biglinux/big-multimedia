// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Video crop controls shared by BigLinux editors.

use adw::prelude::*;
use big_relm4_components::feedback::tooltip;
use relm4::gtk;

/// Translated labels for the four crop spin buttons plus reset/grid toggles.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoCropLabels {
    pub left: String,
    pub right: String,
    pub top: String,
    pub bottom: String,
    pub reset_crop: String,
    pub show_grid_overlay: String,
}

impl BigVideoCropLabels {
    /// Hard-coded English defaults useful for unit tests and previews.
    #[must_use]
    pub fn english() -> Self {
        Self {
            left: "Left".into(),
            right: "Right".into(),
            top: "Top".into(),
            bottom: "Bottom".into(),
            reset_crop: "Reset crop".into(),
            show_grid_overlay: "Show grid overlay".into(),
        }
    }
}

/// Four spin buttons (left/right/top/bottom) plus reset and grid-overlay toggles.
#[derive(Debug, Clone)]
pub struct BigVideoCropControls {
    root: gtk::Box,
    left_spin: gtk::SpinButton,
    right_spin: gtk::SpinButton,
    top_spin: gtk::SpinButton,
    bottom_spin: gtk::SpinButton,
    reset_button: gtk::Button,
    grid_button: gtk::ToggleButton,
}

impl BigVideoCropControls {
    /// Build the crop widget tree with translated `labels`.
    #[must_use]
    pub fn new(labels: BigVideoCropLabels) -> Self {
        let grid = gtk::Grid::builder()
            .row_spacing(4)
            .column_spacing(8)
            .build();
        let left_spin = crop_spin();
        let right_spin = crop_spin();
        let top_spin = crop_spin();
        let bottom_spin = crop_spin();

        grid.attach(&dim_label(&labels.left), 0, 0, 1, 1);
        grid.attach(&left_spin, 1, 0, 1, 1);
        grid.attach(&dim_label(&labels.right), 2, 0, 1, 1);
        grid.attach(&right_spin, 3, 0, 1, 1);
        grid.attach(&dim_label(&labels.top), 0, 1, 1, 1);
        grid.attach(&top_spin, 1, 1, 1, 1);
        grid.attach(&dim_label(&labels.bottom), 2, 1, 1, 1);
        grid.attach(&bottom_spin, 3, 1, 1, 1);

        let reset_button = tooltip::icon_button(
            "edit-undo-symbolic",
            &labels.reset_crop,
            &["flat", "circular"],
        );
        reset_button.set_valign(gtk::Align::Center);

        let grid_button = gtk::ToggleButton::builder()
            .icon_name("view-grid-symbolic")
            .css_classes(["flat", "circular"])
            .valign(gtk::Align::Center)
            .build();
        tooltip::set(&grid_button, &labels.show_grid_overlay);
        grid_button.update_property(&[gtk::accessible::Property::Label(&labels.show_grid_overlay)]);

        let root = gtk::Box::builder().spacing(8).build();
        root.append(&grid);
        root.append(&reset_button);
        root.append(&grid_button);

        Self {
            root,
            left_spin,
            right_spin,
            top_spin,
            bottom_spin,
            reset_button,
            grid_button,
        }
    }

    /// Borrow the root [`gtk::Box`].
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Borrow the "Left" spin button.
    #[must_use]
    pub fn left_spin(&self) -> &gtk::SpinButton {
        &self.left_spin
    }

    /// Borrow the "Right" spin button.
    #[must_use]
    pub fn right_spin(&self) -> &gtk::SpinButton {
        &self.right_spin
    }

    /// Borrow the "Top" spin button.
    #[must_use]
    pub fn top_spin(&self) -> &gtk::SpinButton {
        &self.top_spin
    }

    /// Borrow the "Bottom" spin button.
    #[must_use]
    pub fn bottom_spin(&self) -> &gtk::SpinButton {
        &self.bottom_spin
    }

    /// Borrow the "Reset crop" button.
    #[must_use]
    pub fn reset_button(&self) -> &gtk::Button {
        &self.reset_button
    }

    /// Borrow the grid-overlay toggle.
    #[must_use]
    pub fn grid_button(&self) -> &gtk::ToggleButton {
        &self.grid_button
    }
}

fn crop_spin() -> gtk::SpinButton {
    let adjustment = gtk::Adjustment::new(0.0, 0.0, 9999.0, 1.0, 10.0, 0.0);
    gtk::SpinButton::builder()
        .adjustment(&adjustment)
        .digits(0)
        .width_chars(5)
        .build()
}

fn dim_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .css_classes(["dim-label", "caption"])
        .halign(gtk::Align::End)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_labels_cover_all_crop_controls() {
        let labels = BigVideoCropLabels::english();
        assert_eq!(labels.left, "Left");
        assert_eq!(labels.right, "Right");
        assert_eq!(labels.top, "Top");
        assert_eq!(labels.bottom, "Bottom");
        assert_eq!(labels.reset_crop, "Reset crop");
        assert_eq!(labels.show_grid_overlay, "Show grid overlay");
    }
}
