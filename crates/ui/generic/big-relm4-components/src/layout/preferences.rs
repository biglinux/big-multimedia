// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Preferences page group helpers.

use adw::prelude::*;

/// Display-free specification describing big preferences group behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPreferencesGroupSpec {
    /// Title.
    pub title: String,
    /// Description.
    pub description: Option<String>,
    /// Visible.
    pub visible: bool,
}

impl BigPreferencesGroupSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            visible: true,
        }
    }

    /// Creates a group without a title for button strips or compact controls.
    #[must_use]
    pub fn untitled() -> Self {
        Self::new("")
    }

    /// Configure the `description` setting and return the updated builder.
    ///
    /// The supplied `description` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigPreferencesGroupSpec`].
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Configure the `visible` setting and return the updated builder.
    ///
    /// The supplied `visible` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigPreferencesGroupSpec`].
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Finalize the builder and produce the configured value.
    #[must_use]
    pub fn build(&self) -> adw::PreferencesGroup {
        let mut builder = adw::PreferencesGroup::builder().title(&self.title);
        if let Some(description) = &self.description {
            builder = builder.description(description);
        }
        let group = builder.build();
        group.set_visible(self.visible);
        group
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_group_spec_defaults_visible() {
        let spec = BigPreferencesGroupSpec::new("Video").description("Settings");

        assert_eq!(spec.title, "Video");
        assert_eq!(spec.description.as_deref(), Some("Settings"));
        assert!(spec.visible);
    }

    #[test]
    fn preference_group_spec_can_hide_group() {
        let spec = BigPreferencesGroupSpec::new("Advanced").visible(false);

        assert!(!spec.visible);
    }

    #[test]
    fn preference_group_spec_can_be_untitled() {
        let spec = BigPreferencesGroupSpec::untitled();

        assert_eq!(spec.title, "");
        assert_eq!(spec.description, None);
        assert!(spec.visible);
    }
}
