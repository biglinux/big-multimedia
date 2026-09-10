//! Shared video re-encode options (container / codec / quality).
//!
//! The full converter sidebar and the player's trim mode both let the user
//! re-encode through the same engine — this is the one place the *option
//! vocabulary* is assembled so "Re-encode" offers identical choices everywhere:
//! the user-visible labels come from [`big_media_runtime::ui_labels`], the
//! runtime IDs from the converter sidebar tables. Read [`EncodeOptions::choice`]
//! into an [`EncodeChoice`], then [`EncodeChoice::to_quick_choice`] for a
//! single-file export.

use adw::prelude::*;
use big_media_runtime::converter::profiles::VideoFilters;
use big_media_runtime::ui_labels;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

use crate::combos::{
    CODEC_IDS, CONTAINER_IDS, QUALITY_IDS, as_str_refs, combo_id, make_combo, set_combo_by_id,
};
use crate::export::{QuickExportChoice, QuickExportFormat};
use crate::i18n::i18n;

/// A resolved video re-encode selection (runtime IDs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodeChoice {
    /// Output container id, e.g. `"mp4"`.
    pub container: String,
    /// Video codec id, e.g. `"h264"` (`"copy"` = passthrough).
    pub codec: String,
    /// Quality preset id, e.g. `"good"`.
    pub quality: String,
}

impl EncodeChoice {
    /// Map to a single-file [`QuickExportChoice`] carrying `filters`. Audio is
    /// copied by default (matches the converter's video-export default).
    #[must_use]
    pub fn to_quick_choice(&self, filters: VideoFilters) -> QuickExportChoice {
        QuickExportChoice {
            format: QuickExportFormat::Video {
                container: self.container.clone(),
                codec: self.codec.clone(),
                quality: self.quality.clone(),
            },
            filters,
        }
    }
}

/// Container/codec/quality combo group + a typed reader. Mount [`Self::group`]
/// into any preferences surface; read the live selection with [`Self::choice`].
pub struct EncodeOptions {
    group: adw::PreferencesGroup,
    container_combo: adw::ComboRow,
    codec_combo: adw::ComboRow,
    quality_combo: adw::ComboRow,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl EncodeOptions {
    /// Build the option group with the shared label set, defaulting to
    /// H.264 / Good / MP4 (the converter's sensible default re-encode).
    #[must_use]
    pub fn new() -> Self {
        let container_combo = make_combo(
            &i18n("Format"),
            &as_str_refs(&ui_labels::container_labels()),
        );
        let codec_combo = make_combo(&i18n("Codec"), &as_str_refs(&ui_labels::codec_labels()));
        let quality_combo =
            make_combo(&i18n("Quality"), &as_str_refs(&ui_labels::quality_labels()));

        set_combo_by_id(&container_combo, CONTAINER_IDS, "mp4");
        set_combo_by_id(&codec_combo, CODEC_IDS, "h264");
        set_combo_by_id(&quality_combo, QUALITY_IDS, "good");

        let group = BigPreferencesGroupSpec::new(i18n("Re-encode options"))
            .description(i18n(
                "Choose the file type, video codec, and quality used to create the new clip.",
            ))
            .build();
        group.add(&container_combo);
        group.add(&codec_combo);
        group.add(&quality_combo);

        Self {
            group,
            container_combo,
            codec_combo,
            quality_combo,
        }
    }

    /// The mountable preferences group.
    #[must_use]
    pub fn group(&self) -> &adw::PreferencesGroup {
        &self.group
    }

    /// The live container/codec/quality selection.
    #[must_use]
    pub fn choice(&self) -> EncodeChoice {
        EncodeChoice {
            container: combo_id(&self.container_combo, CONTAINER_IDS),
            codec: combo_id(&self.codec_combo, CODEC_IDS),
            quality: combo_id(&self.quality_combo, QUALITY_IDS),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EncodeChoice;
    use crate::export::QuickExportFormat;
    use big_media_runtime::converter::profiles::VideoFilters;

    #[test]
    fn encode_choice_maps_to_quick_video_format() {
        let choice = EncodeChoice {
            container: "mkv".into(),
            codec: "h265".into(),
            quality: "high".into(),
        };
        let quick = choice.to_quick_choice(VideoFilters::default());
        match quick.format {
            QuickExportFormat::Video {
                container,
                codec,
                quality,
            } => {
                assert_eq!(container, "mkv");
                assert_eq!(codec, "h265");
                assert_eq!(quality, "high");
            }
            QuickExportFormat::Audio { .. } => panic!("expected a video format"),
        }
    }
}
