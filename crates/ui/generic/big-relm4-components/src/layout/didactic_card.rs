// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Standard BigLinux didactic cards for settings and onboarding UI.

use std::cell::RefCell;
use std::collections::HashMap;

use adw::prelude::*;
use relm4::gtk;

thread_local! {
    // Decoded illustrations are immutable and shareable. Cache one GdkPaintable
    // per resource path so every card (and every dialog re-open, across the
    // process) reuses it instead of re-decoding the SVG on each build — that
    // per-build decode was never freed and leaked MBs per dialog open. Bounded:
    // one entry per distinct illustration in the binary's gresource.
    static ILLUSTRATION_PAINTABLES: RefCell<HashMap<String, gtk::gdk::Paintable>> =
        RefCell::new(HashMap::new());
}

/// DEFAULT DIDACTIC CARD SPACING constant.
pub const DEFAULT_DIDACTIC_CARD_SPACING: i32 = 18;
/// DEFAULT DIDACTIC TEXT SPACING constant.
pub const DEFAULT_DIDACTIC_TEXT_SPACING: i32 = 4;
/// DEFAULT DIDACTIC DESCRIPTION MAX CHARS constant.
pub const DEFAULT_DIDACTIC_DESCRIPTION_MAX_CHARS: i32 = 76;

/// Visual layout flavour selected when realising a
/// [`BigDidacticCardSpec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigDidacticCardLayout {
    /// Illustration on the left, text + optional control on the right.
    Horizontal,
    /// Plain card with no illustration at all.
    Plain,
}

/// Display-free specification describing big didactic card behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigDidacticCardSpec {
    /// Resource prefix.
    pub resource_prefix: String,
    /// Illustration name.
    pub illustration_name: Option<String>,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Layout.
    pub layout: BigDidacticCardLayout,
    /// Card spacing.
    pub card_spacing: i32,
    /// Text spacing.
    pub text_spacing: i32,
    /// Description max chars.
    pub description_max_chars: i32,
}

impl BigDidacticCardSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(
        resource_prefix: impl Into<String>,
        illustration_name: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            resource_prefix: resource_prefix.into(),
            illustration_name: Some(illustration_name.into()),
            title: title.into(),
            description: description.into(),
            layout: BigDidacticCardLayout::Horizontal,
            card_spacing: DEFAULT_DIDACTIC_CARD_SPACING,
            text_spacing: DEFAULT_DIDACTIC_TEXT_SPACING,
            description_max_chars: DEFAULT_DIDACTIC_DESCRIPTION_MAX_CHARS,
        }
    }

    /// Build a plain-layout card with no illustration. Useful for
    /// onboarding flows where art is missing.
    #[must_use]
    pub fn plain(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            resource_prefix: String::new(),
            illustration_name: None,
            title: title.into(),
            description: description.into(),
            layout: BigDidacticCardLayout::Plain,
            card_spacing: DEFAULT_DIDACTIC_CARD_SPACING,
            text_spacing: DEFAULT_DIDACTIC_TEXT_SPACING,
            description_max_chars: DEFAULT_DIDACTIC_DESCRIPTION_MAX_CHARS,
        }
    }

    /// Join `resource_prefix` with `illustration_name` into the full
    /// GResource path. Returns `None` when the spec carries no
    /// illustration.
    #[must_use]
    pub fn resource_path(&self) -> Option<String> {
        self.illustration_name
            .as_ref()
            .map(|name| format!("{}/{}", self.resource_prefix.trim_end_matches('/'), name))
    }
}

/// Realised widget wrapper around a [`BigDidacticCardSpec`].
#[derive(Debug, Clone)]
pub struct BigDidacticCard {
    root: gtk::Box,
}

impl BigDidacticCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDidacticCardSpec, control: Option<&impl IsA<gtk::Widget>>) -> Self {
        let root = match spec.layout {
            BigDidacticCardLayout::Horizontal => horizontal_card(&spec, control),
            BigDidacticCardLayout::Plain => plain_card(&spec, control),
        };
        apply_a11y(&root, &spec.title);
        Self { root }
    }

    /// Consume `self` and yield the underlying root.
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }
}

/// Convenience: build a card with an embedded control and return its
/// root box.
#[must_use]
pub fn didactic_card(spec: BigDidacticCardSpec, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    BigDidacticCard::new(spec, Some(control)).into_root()
}

fn horizontal_card<T: IsA<gtk::Widget>>(
    spec: &BigDidacticCardSpec,
    control: Option<&T>,
) -> gtk::Box {
    let card = card_root(gtk::Orientation::Horizontal, spec.card_spacing);
    if let Some(picture) = illustration(spec) {
        card.append(&picture);
    }
    card.append(&text_column(spec));
    if let Some(control) = control {
        control.as_ref().set_valign(gtk::Align::Center);
        card.append(control);
    }
    card
}

fn plain_card<T: IsA<gtk::Widget>>(spec: &BigDidacticCardSpec, control: Option<&T>) -> gtk::Box {
    let card = card_root(gtk::Orientation::Horizontal, spec.card_spacing);
    card.append(&text_column(spec));
    if let Some(control) = control {
        control.as_ref().set_valign(gtk::Align::Center);
        card.append(control);
    }
    card
}

fn card_root(orientation: gtk::Orientation, spacing: i32) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(orientation)
        .spacing(spacing)
        .hexpand(true)
        .build();
    card.add_css_class("didactic-card");
    card
}

fn illustration(spec: &BigDidacticCardSpec) -> Option<gtk::Picture> {
    let path = spec.resource_path()?;
    // Reuse a cached, already-decoded paintable when we have one; otherwise
    // decode once via `for_resource` and cache that paintable for next time.
    let cached = ILLUSTRATION_PAINTABLES.with(|c| c.borrow().get(&path).cloned());
    let picture = if let Some(paintable) = cached {
        gtk::Picture::for_paintable(&paintable)
    } else {
        let picture = gtk::Picture::for_resource(&path);
        if let Some(paintable) = picture.paintable() {
            ILLUSTRATION_PAINTABLES.with(|c| {
                c.borrow_mut().insert(path.clone(), paintable);
            });
        }
        picture
    };
    picture.add_css_class("didactic-illustration");
    picture.set_valign(gtk::Align::Center);
    picture.set_accessible_role(gtk::AccessibleRole::Presentation);
    Some(picture)
}

fn text_column(spec: &BigDidacticCardSpec) -> gtk::Box {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(spec.text_spacing)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();

    let title = gtk::Label::builder()
        .label(&spec.title)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["didactic-title"])
        .build();
    column.append(&title);

    if !spec.description.is_empty() {
        let description = gtk::Label::builder()
            .label(&spec.description)
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(spec.description_max_chars)
            .css_classes(["didactic-description"])
            .build();
        column.append(&description);
    }

    column
}

fn apply_a11y(card: &gtk::Box, title: &str) {
    card.set_accessible_role(gtk::AccessibleRole::Group);
    card.update_property(&[gtk::accessible::Property::Label(title)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_path_trims_trailing_slash() {
        let spec = BigDidacticCardSpec::new("/app/illustrations/", "x.svg", "Title", "Body");
        assert_eq!(
            spec.resource_path().as_deref(),
            Some("/app/illustrations/x.svg")
        );
    }

    #[test]
    fn plain_card_has_no_resource_path() {
        let spec = BigDidacticCardSpec::plain("Title", "Body");
        assert_eq!(spec.resource_path(), None);
        assert_eq!(spec.layout, BigDidacticCardLayout::Plain);
    }
}
