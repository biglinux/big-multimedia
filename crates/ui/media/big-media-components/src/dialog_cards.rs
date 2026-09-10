//! Illustrated dialog card helpers for media settings dialogs.
//!
//! `BigMediaDialogCards` binds an SVG resource prefix and produces
//! illustration cards backed by [`big_relm4_components::layout::illustration_card`].
//! The wrapper exists so an app can ship its own resource bundle while
//! reusing the layout helpers.

use adw::prelude::*;
use big_relm4_components::input::dropdown::{dropdown_from_strings, sync_dropdown_with_combo_row};
use big_relm4_components::layout::illustration_card::{
    BigIllustrationCard, BigIllustrationCardSpec,
};
use relm4::gtk;

/// A factory for illustrated dialog cards that load SVGs from a fixed resource prefix.
///
/// The prefix is appended to every filename passed to [`load_svg`](Self::load_svg)
/// and to the `BigIllustrationCardSpec` produced by [`card`](Self::card) and
/// [`card_with_extra`](Self::card_with_extra). Construct one per app and
/// keep it as a `const` so the SVG path is locked at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigMediaDialogCards {
    resource_prefix: &'static str,
}

impl BigMediaDialogCards {
    /// Bind the card factory to a GResource path prefix (must end in `/`).
    #[must_use]
    pub const fn new(resource_prefix: &'static str) -> Self {
        Self { resource_prefix }
    }

    /// Build the full resource path for an illustration filename.
    #[must_use]
    pub fn illustration_resource_path(self, filename: &str) -> String {
        format!("{}{}", self.resource_prefix, filename)
    }

    /// Build the display-free illustration-card spec for this media resource prefix.
    #[must_use]
    pub fn illustration_card_spec(
        self,
        svg: &str,
        title: &str,
        description: &str,
    ) -> BigIllustrationCardSpec {
        BigIllustrationCardSpec::new(self.resource_prefix, svg, title, description)
    }

    /// Load `filename` from the bound resource prefix into a sized [`gtk::Picture`].
    ///
    /// The returned picture has a 92×68 size request, `can-shrink` enabled,
    /// and content-fit set to `Contain` so it fits dialog headings.
    #[must_use]
    pub fn load_svg(self, filename: &str) -> gtk::Picture {
        let path = self.illustration_resource_path(filename);
        let pic = gtk::Picture::for_resource(&path);
        pic.set_size_request(92, 68);
        pic.set_can_shrink(true);
        pic.set_content_fit(gtk::ContentFit::Contain);
        pic
    }

    /// Build an illustration card with an SVG, title, description, and one control widget.
    #[must_use]
    pub fn card(
        self,
        svg: &str,
        title: &str,
        description: &str,
        control: &impl IsA<gtk::Widget>,
    ) -> gtk::Box {
        let spec = self.illustration_card_spec(svg, title, description);
        BigIllustrationCard::new(spec, control).into_root()
    }

    /// Build an illustration card that hosts a primary control plus an extra widget below.
    #[must_use]
    pub fn card_with_extra(
        self,
        svg: &str,
        title: &str,
        description: &str,
        control: &impl IsA<gtk::Widget>,
        extra: &impl IsA<gtk::Widget>,
    ) -> gtk::Box {
        let spec = self.illustration_card_spec(svg, title, description);
        BigIllustrationCard::new_with_extra(spec, control, Some(extra)).into_root()
    }
}

/// Build a [`gtk::DropDown`] backed by `labels`.
#[must_use]
pub fn make_dropdown(labels: &[&str]) -> gtk::DropDown {
    dropdown_from_strings(labels)
}

/// Two-way bind a dialog [`gtk::DropDown`] to a source [`adw::ComboRow`].
///
/// Selection changes propagate in both directions so the dialog's dropdown
/// and the underlying row stay synchronized while the dialog is open.
pub fn sync_dropdowns(dialog_dd: &gtk::DropDown, source_combo: &adw::ComboRow) {
    sync_dropdown_with_combo_row(dialog_dd, source_combo);
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_relm4_components::layout::illustration_card::BigIllustrationSource;

    #[test]
    fn keeps_resource_prefix_contract() {
        let cards = BigMediaDialogCards::new("/prefix/");
        assert_eq!(cards.resource_prefix, "/prefix/");
    }

    #[test]
    fn builds_illustration_resource_paths_from_prefix() {
        let cards = BigMediaDialogCards::new("/org/biglinux/media/illustrations/");

        assert_eq!(
            cards.illustration_resource_path("noise.svg"),
            "/org/biglinux/media/illustrations/noise.svg"
        );
    }

    #[test]
    fn builds_display_free_card_spec_from_prefix() {
        let cards = BigMediaDialogCards::new("/org/biglinux/media/illustrations/");
        let spec = cards.illustration_card_spec(
            "cleanup.svg",
            "Audio cleanup",
            "Reduce noise before exporting.",
        );
        let resolved = spec.resolved();

        assert_eq!(
            spec.source,
            BigIllustrationSource::Resource(
                "/org/biglinux/media/illustrations/cleanup.svg".to_string()
            )
        );
        assert_eq!(spec.resource_prefix, "/org/biglinux/media/illustrations/");
        assert_eq!(spec.illustration_name, "cleanup.svg");
        assert_eq!(spec.title, "Audio cleanup");
        assert_eq!(spec.description, "Reduce noise before exporting.");
        assert_eq!(
            resolved.resource_path,
            "/org/biglinux/media/illustrations/cleanup.svg"
        );
        assert_eq!(resolved.picture_width, 92);
        assert_eq!(resolved.picture_height, 68);
        assert_eq!(resolved.inner_margin, 16);
        assert_eq!(resolved.outer_margin_bottom, 12);
    }
}
