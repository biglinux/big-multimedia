// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Switch helpers.

use relm4::gtk;

use crate::feedback::tooltip;

/// Data used to build a standalone switch control.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSwitchControlSpec {
    /// Accessible label exposed for assistive technology.
    pub accessible_label: String,
    /// Tooltip text. Defaults to the accessible label.
    pub tooltip: Option<String>,
    /// Initial active state.
    pub active: bool,
}

impl BigSwitchControlSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(accessible_label: impl Into<String>, active: bool) -> Self {
        Self {
            accessible_label: accessible_label.into(),
            tooltip: None,
            active,
        }
    }

    /// Configure the tooltip and return the updated builder.
    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigSwitchControlResolved {
        BigSwitchControlResolved {
            accessible_label: self.accessible_label.clone(),
            tooltip: self.tooltip.clone(),
            active: self.active,
        }
    }
}

/// Pure resolved switch contract. Safe to test without GTK/display.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSwitchControlResolved {
    /// Accessible label exposed for assistive technology.
    pub accessible_label: String,
    /// Tooltip text. When absent, use [`Self::accessible_label`].
    pub tooltip: Option<String>,
    /// Initial active state.
    pub active: bool,
}

impl BigSwitchControlResolved {
    /// Return the tooltip text that should be attached to the switch.
    #[must_use]
    pub fn tooltip_label(&self) -> &str {
        self.tooltip.as_deref().unwrap_or(&self.accessible_label)
    }
}

/// Built standalone switch control.
#[derive(Debug, Clone)]
pub struct BigSwitchControl {
    root: gtk::Switch,
}

impl BigSwitchControl {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigSwitchControlSpec) -> Self {
        use relm4::gtk::prelude::*;

        let resolved = spec.resolved();
        let root = gtk::Switch::builder()
            .active(resolved.active)
            .valign(gtk::Align::Center)
            .build();
        tooltip::set(&root, resolved.tooltip_label());
        root.update_property(&[gtk::accessible::Property::Label(&resolved.accessible_label)]);
        Self { root }
    }

    /// Return a reference to the root switch.
    #[must_use]
    pub fn root(&self) -> &gtk::Switch {
        &self.root
    }

    /// Consume `self` and yield the underlying switch.
    #[must_use]
    pub fn into_root(self) -> gtk::Switch {
        self.root
    }
}
