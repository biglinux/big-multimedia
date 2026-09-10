// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Compact media metadata grid.

use adw::prelude::*;
use relm4::gtk;

/// Translated key labels for [`BigMediaMetadataGrid`].
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigMediaMetadataLabels {
    pub resolution: String,
    pub codec: String,
    pub size: String,
    pub duration: String,
}

impl BigMediaMetadataLabels {
    /// Hard-coded English defaults useful for unit tests and previews.
    #[must_use]
    pub fn english() -> Self {
        Self {
            resolution: "Resolution:".into(),
            codec: "Codec:".into(),
            size: "Size:".into(),
            duration: "Duration:".into(),
        }
    }
}

/// Compact `key / value` grid showing four media metadata rows.
///
/// Useful as a footer in the video editor. The caller updates each value
/// label through the accessors when probe metadata lands.
#[derive(Debug, Clone)]
pub struct BigMediaMetadataGrid {
    root: gtk::Grid,
    resolution_value: gtk::Label,
    codec_value: gtk::Label,
    size_value: gtk::Label,
    duration_value: gtk::Label,
}

impl BigMediaMetadataGrid {
    /// Build the grid with translated `labels`.
    #[must_use]
    pub fn new(labels: BigMediaMetadataLabels) -> Self {
        let root = gtk::Grid::builder()
            .row_spacing(2)
            .column_spacing(8)
            .hexpand(true)
            .build();

        let resolution_value = value_label();
        let codec_value = value_label();
        let size_value = value_label();
        let duration_value = value_label();

        root.attach(&key_label(&labels.resolution), 0, 0, 1, 1);
        root.attach(&resolution_value, 1, 0, 1, 1);
        root.attach(&key_label(&labels.codec), 0, 1, 1, 1);
        root.attach(&codec_value, 1, 1, 1, 1);
        root.attach(&key_label(&labels.size), 0, 2, 1, 1);
        root.attach(&size_value, 1, 2, 1, 1);
        root.attach(&key_label(&labels.duration), 0, 3, 1, 1);
        root.attach(&duration_value, 1, 3, 1, 1);

        Self {
            root,
            resolution_value,
            codec_value,
            size_value,
            duration_value,
        }
    }

    /// Borrow the root [`gtk::Grid`] for embedding.
    #[must_use]
    pub fn root(&self) -> &gtk::Grid {
        &self.root
    }

    /// Borrow the resolution-value label so the caller can set its text.
    #[must_use]
    pub fn resolution_value(&self) -> &gtk::Label {
        &self.resolution_value
    }

    /// Borrow the codec-value label so the caller can set its text.
    #[must_use]
    pub fn codec_value(&self) -> &gtk::Label {
        &self.codec_value
    }

    /// Borrow the size-value label so the caller can set its text.
    #[must_use]
    pub fn size_value(&self) -> &gtk::Label {
        &self.size_value
    }

    /// Borrow the duration-value label so the caller can set its text.
    #[must_use]
    pub fn duration_value(&self) -> &gtk::Label {
        &self.duration_value
    }
}

fn key_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .css_classes(["caption", "heading"])
        .halign(gtk::Align::Start)
        .build()
}

fn value_label() -> gtk::Label {
    gtk::Label::builder()
        .label("...")
        .css_classes(["dim-label", "caption"])
        .halign(gtk::Align::Start)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_labels_keep_expected_order() {
        let labels = BigMediaMetadataLabels::english();
        assert_eq!(labels.resolution, "Resolution:");
        assert_eq!(labels.codec, "Codec:");
        assert_eq!(labels.size, "Size:");
        assert_eq!(labels.duration, "Duration:");
    }
}
