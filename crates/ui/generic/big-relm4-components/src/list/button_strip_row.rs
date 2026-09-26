// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux action row with a compact suffix button strip.

use adw::prelude::*;
use relm4::gtk;

use super::row_core::row_text;

/// Data used to build a compact button-strip row.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigButtonStripRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Spacing.
    pub spacing: i32,
    /// Activatable.
    pub activatable: bool,
    /// Allow markup.
    pub allow_markup: bool,
}

impl BigButtonStripRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            prefix_icon_name: None,
            spacing: 4,
            activatable: false,
            allow_markup: false,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigButtonStripRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `prefix_icon_name` setting and return the updated builder.
    ///
    /// The supplied `icon_name` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigButtonStripRowSpec`].
    #[must_use]
    pub fn prefix_icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.prefix_icon_name = Some(icon_name.into());
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigButtonStripRowResolved {
        BigButtonStripRowResolved {
            title: row_text(&self.title, self.allow_markup),
            subtitle: self
                .subtitle
                .as_ref()
                .map(|subtitle| row_text(subtitle, self.allow_markup)),
            prefix_icon_name: self.prefix_icon_name.clone(),
            spacing: self.spacing.max(0),
            activatable: self.activatable,
        }
    }
}

/// Pure resolved row contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigButtonStripRowResolved {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Prefix icon name.
    pub prefix_icon_name: Option<String>,
    /// Spacing.
    pub spacing: i32,
    /// Activatable.
    pub activatable: bool,
}

/// Built row plus its button strip.
#[derive(Debug, Clone)]
pub struct BigButtonStripRow {
    root: adw::ActionRow,
    strip: gtk::Box,
}

impl BigButtonStripRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigButtonStripRowSpec) -> Self {
        let resolved = spec.resolved();
        let mut builder = adw::ActionRow::builder()
            .title(resolved.title.as_str())
            .activatable(resolved.activatable);

        if let Some(subtitle) = resolved.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }

        let root = builder.build();

        if let Some(icon_name) = resolved.prefix_icon_name.as_deref() {
            root.add_prefix(&gtk::Image::from_icon_name(icon_name));
        }

        let strip = gtk::Box::builder()
            .spacing(resolved.spacing)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        root.add_suffix(&strip);

        Self { root, strip }
    }

    /// Append `widget` to the horizontal button strip.
    pub fn append(&self, widget: &impl IsA<gtk::Widget>) {
        self.strip.append(widget);
    }

    /// Return a reference to the `root` exposed by this [`BigButtonStripRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &adw::ActionRow {
        &self.root
    }

    /// Consume `self` and yield the underlying root.
    #[must_use]
    pub fn into_root(self) -> adw::ActionRow {
        self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_defaults_to_plain_title_and_compact_spacing() {
        let spec = BigButtonStripRowSpec::new("Transform");

        assert_eq!(spec.title, "Transform");
        assert_eq!(spec.subtitle, None);
        assert_eq!(spec.prefix_icon_name, None);
        assert_eq!(spec.spacing, 4);
        assert!(!spec.activatable);
        assert!(!spec.allow_markup);
    }

    #[test]
    fn resolved_escapes_markup_by_default() {
        let resolved = BigButtonStripRowSpec::new("<b>Transform</b>")
            .subtitle("<i>Duration</i>")
            .resolved();

        assert_eq!(resolved.title, "&lt;b&gt;Transform&lt;/b&gt;");
        assert_eq!(
            resolved.subtitle.as_deref(),
            Some("&lt;i&gt;Duration&lt;/i&gt;")
        );
    }
}
