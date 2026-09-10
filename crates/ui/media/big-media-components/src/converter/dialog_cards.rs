//! Shared illustrated card helpers for media converter dialogs.
//!
//! Wraps [`crate::dialog_cards::BigMediaDialogCards`] with the converter's
//! own GResource prefix and re-exports the dropdown helpers so converter
//! dialogs only need one import.

use adw::prelude::*;
use relm4::gtk;

use crate::dialog_cards::BigMediaDialogCards;
pub use crate::dialog_cards::{make_dropdown, sync_dropdowns};

const CONVERTER_ILLUSTRATION_PREFIX: &str = "/br/com/biglinux/VideoPlayer/illustrations/";
const CARDS: BigMediaDialogCards = BigMediaDialogCards::new(CONVERTER_ILLUSTRATION_PREFIX);

/// Load an illustration SVG from the converter resource bundle.
#[must_use]
pub fn load_svg(filename: &str) -> gtk::Picture {
    CARDS.load_svg(filename)
}

/// Build an illustration card with one control widget.
#[must_use]
pub fn make_card(
    svg: &str,
    title: &str,
    description: &str,
    control: &impl IsA<gtk::Widget>,
) -> gtk::Box {
    CARDS.card(svg, title, description, control)
}

/// Build an illustration card with a primary control and an extra widget below.
#[must_use]
pub fn make_card_with_extra(
    svg: &str,
    title: &str,
    description: &str,
    control: &impl IsA<gtk::Widget>,
    extra: &impl IsA<gtk::Widget>,
) -> gtk::Box {
    CARDS.card_with_extra(svg, title, description, control, extra)
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_relm4_components::layout::illustration_card::BigIllustrationSource;

    #[test]
    fn converter_cards_use_existing_video_resource_bundle() {
        assert_eq!(
            CONVERTER_ILLUSTRATION_PREFIX,
            "/br/com/biglinux/VideoPlayer/illustrations/"
        );
    }

    #[test]
    fn converter_cards_build_resource_paths_from_video_bundle() {
        assert_eq!(
            CARDS.illustration_resource_path("audio_codec.svg"),
            "/br/com/biglinux/VideoPlayer/illustrations/audio_codec.svg"
        );
    }

    #[test]
    fn converter_cards_build_display_free_specs_from_video_bundle() {
        let spec = CARDS.illustration_card_spec(
            "audio_bitrate.svg",
            "Audio bitrate",
            "Higher bitrate means better audio quality.",
        );
        let resolved = spec.resolved();

        assert_eq!(
            spec.source,
            BigIllustrationSource::Resource(
                "/br/com/biglinux/VideoPlayer/illustrations/audio_bitrate.svg".to_string()
            )
        );
        assert_eq!(
            spec.resource_prefix,
            "/br/com/biglinux/VideoPlayer/illustrations/"
        );
        assert_eq!(spec.illustration_name, "audio_bitrate.svg");
        assert_eq!(spec.title, "Audio bitrate");
        assert_eq!(
            spec.description,
            "Higher bitrate means better audio quality."
        );
        assert_eq!(
            resolved.resource_path,
            "/br/com/biglinux/VideoPlayer/illustrations/audio_bitrate.svg"
        );
        assert_eq!(resolved.picture_width, 92);
        assert_eq!(resolved.picture_height, 68);
    }
}
