//! Video encoding dialog for media converters.

#![allow(clippy::needless_borrows_for_generic_args)]

use adw::prelude::*;
use big_relm4_components::input::revealed_entry_row::{
    BigRevealedEntryRow, BigRevealedEntryRowSpec,
};
use big_relm4_components::layout::dialog_page::BigDialogPage;
use relm4::gtk;

use crate::converter::dialog_cards::{make_card, make_dropdown, sync_dropdowns};

/// Translated strings used by the video encoding dialog cards.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoEncodingDialogText {
    pub title: String,
    pub description: String,
    pub output_format_title: String,
    pub output_format_description: String,
    pub codec_title: String,
    pub codec_description: String,
    pub quality_title: String,
    pub quality_description: String,
    pub resolution_title: String,
    pub resolution_description: String,
    pub speed_title: String,
    pub speed_description: String,
    pub software_decode_title: String,
    pub software_decode_description: String,
    pub custom_label: String,
}

impl BigVideoEncodingDialogText {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            title: text("Video Encoding"),
            description: text(
                "Configure how your video will be encoded. The defaults work well for most cases.",
            ),
            output_format_title: text("Output Format"),
            output_format_description: text(
                "The container that holds audio and video. MP4 is the most compatible. MKV supports all codecs.",
            ),
            codec_title: text("Video Codec"),
            codec_description: text(
                "Choose the compression format. H.264 is the most compatible. H.265 and AV1 produce smaller files but need more processing power.",
            ),
            quality_title: text("Image Quality"),
            quality_description: text(
                "Higher quality keeps more detail but creates larger files. 'Good' is a balanced choice for most videos.",
            ),
            resolution_title: text("Output Resolution"),
            resolution_description: text(
                "Increasing beyond the original resolution does not improve quality. Reducing the resolution creates smaller files.",
            ),
            speed_title: text("Conversion Speed"),
            speed_description: text(
                "Slower conversion produces smaller files. Faster presets create larger files but finish sooner.",
            ),
            software_decode_title: text("Software Decode"),
            software_decode_description: text(
                "Use CPU for decoding instead of GPU. Enable if you see visual artifacts with GPU decoding.",
            ),
            custom_label: text("Custom:"),
        }
    }
}

/// References to source widgets mirrored by the video encoding dialog.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy)]
pub struct BigVideoEncodingDialogControls<'a> {
    pub container_labels: &'a [&'a str],
    pub container_combo: &'a adw::ComboRow,
    pub codec_labels: &'a [&'a str],
    pub codec_combo: &'a adw::ComboRow,
    pub quality_labels: &'a [&'a str],
    pub quality_combo: &'a adw::ComboRow,
    pub resolution_labels: &'a [&'a str],
    pub resolution_option_count: usize,
    pub resolution_combo: &'a adw::ComboRow,
    pub custom_resolution_entry: &'a gtk::Entry,
    pub preset_labels: &'a [&'a str],
    pub preset_combo: &'a adw::ComboRow,
    pub software_decode_switch: &'a adw::SwitchRow,
}

/// Present the video encoding dialog as a modal child of `parent`.
pub fn show_video_encoding_dialog(
    parent: &impl IsA<gtk::Widget>,
    text: BigVideoEncodingDialogText,
    controls: BigVideoEncodingDialogControls<'_>,
) {
    let dialog = adw::Dialog::builder()
        .title(&text.title)
        .content_width(560)
        .content_height(700)
        .build();

    let page = BigDialogPage::scrolled();
    let content = page.content().clone();
    page.append_description(&text.description);

    let format_dropdown = make_dropdown(controls.container_labels);
    sync_dropdowns(&format_dropdown, controls.container_combo);
    content.append(&make_card(
        "output_format.svg",
        &text.output_format_title,
        &text.output_format_description,
        &format_dropdown,
    ));

    let codec_dropdown = make_dropdown(controls.codec_labels);
    sync_dropdowns(&codec_dropdown, controls.codec_combo);
    content.append(&make_card(
        "codec_h264.svg",
        &text.codec_title,
        &text.codec_description,
        &codec_dropdown,
    ));

    let quality_dropdown = make_dropdown(controls.quality_labels);
    sync_dropdowns(&quality_dropdown, controls.quality_combo);
    content.append(&make_card(
        "image_quality.svg",
        &text.quality_title,
        &text.quality_description,
        &quality_dropdown,
    ));

    let resolution_dropdown = make_dropdown(controls.resolution_labels);
    sync_dropdowns(&resolution_dropdown, controls.resolution_combo);
    let resolution_card = make_card(
        "resolution.svg",
        &text.resolution_title,
        &text.resolution_description,
        &resolution_dropdown,
    );
    let entry_row = BigRevealedEntryRow::new(
        BigRevealedEntryRowSpec::last_option(
            text.custom_label.clone(),
            controls.resolution_option_count,
        ),
        &resolution_dropdown,
        controls.custom_resolution_entry,
    )
    .into_root();
    resolution_card.append(&entry_row);
    content.append(&resolution_card);

    let speed_dropdown = make_dropdown(controls.preset_labels);
    sync_dropdowns(&speed_dropdown, controls.preset_combo);
    content.append(&make_card(
        "preset_speed.svg",
        &text.speed_title,
        &text.speed_description,
        &speed_dropdown,
    ));

    let software_decode_switch = gtk::Switch::builder()
        .active(controls.software_decode_switch.is_active())
        .valign(gtk::Align::Center)
        .build();
    software_decode_switch.update_property(&[gtk::accessible::Property::Label(
        &text.software_decode_title,
    )]);
    {
        let source = controls.software_decode_switch.clone();
        software_decode_switch.connect_active_notify(move |switch| {
            source.set_active(switch.is_active());
        });
    }
    content.append(&make_card(
        "sw_decode.svg",
        &text.software_decode_title,
        &text.software_decode_description,
        &software_decode_switch,
    ));

    let toolbar = page.into_root();
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}
