//! Audio encoding dialog for media converters.

#![allow(clippy::needless_borrows_for_generic_args)]

use adw::prelude::*;
use big_relm4_components::input::revealed_entry_row::{
    BigRevealedEntryRow, BigRevealedEntryRowSpec,
};
use big_relm4_components::layout::dialog_page::BigDialogPage;
use relm4::gtk;

use crate::converter::dialog_cards::{make_card, make_dropdown, sync_dropdowns};

const DIALOG_CONTENT_WIDTH: i32 = 560;
const DIALOG_CONTENT_HEIGHT: i32 = 550;
const REENCODE_HANDLING_SELECTED_INDEX: u32 = 1;
const HANDLING_ILLUSTRATION: &str = "audio_handling.svg";
const CODEC_ILLUSTRATION: &str = "audio_codec.svg";
const BITRATE_ILLUSTRATION: &str = "audio_bitrate.svg";
const CHANNELS_ILLUSTRATION: &str = "audio_channels.svg";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AudioEncodingDialogPlan {
    content_width: i32,
    content_height: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AudioEncodingCardSpec {
    illustration_name: &'static str,
    title: String,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioEncodingCardSpecs {
    handling: AudioEncodingCardSpec,
    codec: AudioEncodingCardSpec,
    bitrate: AudioEncodingCardSpec,
    channels: AudioEncodingCardSpec,
    bitrate_custom_entry: BigRevealedEntryRowSpec,
    channels_custom_entry: BigRevealedEntryRowSpec,
}

impl Default for AudioEncodingCardSpecs {
    fn default() -> Self {
        Self {
            handling: AudioEncodingCardSpec::default(),
            codec: AudioEncodingCardSpec::default(),
            bitrate: AudioEncodingCardSpec::default(),
            channels: AudioEncodingCardSpec::default(),
            bitrate_custom_entry: BigRevealedEntryRowSpec::new("", 0),
            channels_custom_entry: BigRevealedEntryRowSpec::new("", 0),
        }
    }
}

/// Translated strings for the audio encoding dialog cards.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAudioEncodingDialogText {
    pub title: String,
    pub description: String,
    pub handling_title: String,
    pub handling_description: String,
    pub codec_title: String,
    pub codec_description: String,
    pub bitrate_title: String,
    pub bitrate_description: String,
    pub channels_title: String,
    pub channels_description: String,
    pub custom_label: String,
}

impl BigAudioEncodingDialogText {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            title: text("Audio"),
            description: text(
                "Configure how the audio track will be handled. Copying keeps the original quality. Re-encoding allows changes.",
            ),
            handling_title: text("Audio Handling"),
            handling_description: text(
                "Choose what to do with the audio track. 'Copy' keeps the original. 'Re-encode' allows changing codec, bitrate and applying audio cleaning.",
            ),
            codec_title: text("Audio Codec"),
            codec_description: text(
                "The format used to compress audio. AAC is widely compatible. Opus offers better quality at lower bitrates. Choose 'Copy' in audio handling to keep the original codec.",
            ),
            bitrate_title: text("Audio Bitrate"),
            bitrate_description: text(
                "Higher bitrate means better audio quality but larger files. 192k is a good balance for most content.",
            ),
            channels_title: text("Audio Channels"),
            channels_description: text(
                "'Default' keeps the original channel count from the source video. Stereo (2) is standard. Mono (1) for voice only. 5.1 (6) for surround sound.",
            ),
            custom_label: text("Custom:"),
        }
    }
}

/// References to the source widgets the audio encoding dialog mirrors.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy)]
pub struct BigAudioEncodingDialogControls<'a> {
    pub handling_labels: &'a [&'a str],
    pub handling_combo: &'a adw::ComboRow,
    pub codec_labels: &'a [&'a str],
    pub codec_combo: &'a adw::ComboRow,
    pub bitrate_labels: &'a [&'a str],
    pub bitrate_option_count: usize,
    pub bitrate_combo: &'a adw::ComboRow,
    pub custom_bitrate_entry: &'a gtk::Entry,
    pub channels_labels: &'a [&'a str],
    pub channels_option_count: usize,
    pub channels_combo: &'a adw::ComboRow,
    pub custom_channels_entry: &'a gtk::Entry,
}

/// Present the audio encoding dialog as a modal child of `parent`.
pub fn show_audio_encoding_dialog(
    parent: &impl IsA<gtk::Widget>,
    text: BigAudioEncodingDialogText,
    controls: BigAudioEncodingDialogControls<'_>,
) {
    let plan = audio_encoding_dialog_plan();
    let card_specs = audio_encoding_card_specs(
        &text,
        controls.bitrate_option_count,
        controls.channels_option_count,
    );

    let dialog = adw::Dialog::builder()
        .title(&text.title)
        .content_width(plan.content_width)
        .content_height(plan.content_height)
        .build();

    let page = BigDialogPage::scrolled();
    let content = page.content().clone();
    page.append_description(&text.description);

    let handling_dropdown = make_dropdown(controls.handling_labels);
    sync_dropdowns(&handling_dropdown, controls.handling_combo);
    content.append(&make_card(
        card_specs.handling.illustration_name,
        &card_specs.handling.title,
        &card_specs.handling.description,
        &handling_dropdown,
    ));

    let codec_dropdown = make_dropdown(controls.codec_labels);
    sync_dropdowns(&codec_dropdown, controls.codec_combo);
    let codec_card = make_card(
        card_specs.codec.illustration_name,
        &card_specs.codec.title,
        &card_specs.codec.description,
        &codec_dropdown,
    );
    content.append(&codec_card);

    let bitrate_dropdown = make_dropdown(controls.bitrate_labels);
    sync_dropdowns(&bitrate_dropdown, controls.bitrate_combo);
    let bitrate_card = make_card(
        card_specs.bitrate.illustration_name,
        &card_specs.bitrate.title,
        &card_specs.bitrate.description,
        &bitrate_dropdown,
    );
    let bitrate_entry_row = BigRevealedEntryRow::new(
        card_specs.bitrate_custom_entry,
        &bitrate_dropdown,
        controls.custom_bitrate_entry,
    )
    .into_root();
    bitrate_card.append(&bitrate_entry_row);
    content.append(&bitrate_card);

    let channels_dropdown = make_dropdown(controls.channels_labels);
    sync_dropdowns(&channels_dropdown, controls.channels_combo);
    let channels_card = make_card(
        card_specs.channels.illustration_name,
        &card_specs.channels.title,
        &card_specs.channels.description,
        &channels_dropdown,
    );
    let channels_entry_row = BigRevealedEntryRow::new(
        card_specs.channels_custom_entry,
        &channels_dropdown,
        controls.custom_channels_entry,
    )
    .into_root();
    channels_card.append(&channels_entry_row);
    content.append(&channels_card);

    sync_encoding_sensitive(
        &handling_dropdown,
        &[codec_card, bitrate_card, channels_card],
    );

    let toolbar = page.into_root();
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

fn sync_encoding_sensitive(handling_dropdown: &gtk::DropDown, cards: &[gtk::Box]) {
    let is_encode = is_reencode_handling_selected(handling_dropdown.selected());
    for card in cards {
        card.set_sensitive(is_encode);
    }

    let cards = cards.to_vec();
    handling_dropdown.connect_selected_notify(move |dropdown| {
        let is_encode = is_reencode_handling_selected(dropdown.selected());
        for card in &cards {
            card.set_sensitive(is_encode);
        }
    });
}

fn audio_encoding_dialog_plan() -> AudioEncodingDialogPlan {
    AudioEncodingDialogPlan {
        content_width: DIALOG_CONTENT_WIDTH,
        content_height: DIALOG_CONTENT_HEIGHT,
    }
}

fn audio_encoding_card_specs(
    text: &BigAudioEncodingDialogText,
    bitrate_option_count: usize,
    channels_option_count: usize,
) -> AudioEncodingCardSpecs {
    AudioEncodingCardSpecs {
        handling: AudioEncodingCardSpec {
            illustration_name: HANDLING_ILLUSTRATION,
            title: text.handling_title.clone(),
            description: text.handling_description.clone(),
        },
        codec: AudioEncodingCardSpec {
            illustration_name: CODEC_ILLUSTRATION,
            title: text.codec_title.clone(),
            description: text.codec_description.clone(),
        },
        bitrate: AudioEncodingCardSpec {
            illustration_name: BITRATE_ILLUSTRATION,
            title: text.bitrate_title.clone(),
            description: text.bitrate_description.clone(),
        },
        channels: AudioEncodingCardSpec {
            illustration_name: CHANNELS_ILLUSTRATION,
            title: text.channels_title.clone(),
            description: text.channels_description.clone(),
        },
        bitrate_custom_entry: BigRevealedEntryRowSpec::last_option(
            text.custom_label.clone(),
            bitrate_option_count,
        ),
        channels_custom_entry: BigRevealedEntryRowSpec::last_option(
            text.custom_label.clone(),
            channels_option_count,
        ),
    }
}

fn is_reencode_handling_selected(selected_index: u32) -> bool {
    selected_index == REENCODE_HANDLING_SELECTED_INDEX
}

#[cfg(test)]
mod tests {
    use super::{
        BITRATE_ILLUSTRATION, BigAudioEncodingDialogText, CHANNELS_ILLUSTRATION,
        CODEC_ILLUSTRATION, DIALOG_CONTENT_HEIGHT, DIALOG_CONTENT_WIDTH, HANDLING_ILLUSTRATION,
        REENCODE_HANDLING_SELECTED_INDEX, audio_encoding_card_specs, audio_encoding_dialog_plan,
        is_reencode_handling_selected,
    };

    #[test]
    fn translated_text_routes_every_visible_message_through_callback() {
        let mut translated_messages = Vec::new();
        let text = BigAudioEncodingDialogText::translated(|message| {
            translated_messages.push(message.to_string());
            format!("translated:{message}")
        });

        assert_eq!(text.title, "translated:Audio");
        assert_eq!(text.handling_title, "translated:Audio Handling");
        assert_eq!(text.codec_title, "translated:Audio Codec");
        assert_eq!(text.bitrate_title, "translated:Audio Bitrate");
        assert_eq!(text.channels_title, "translated:Audio Channels");
        assert_eq!(text.custom_label, "translated:Custom:");
        assert_eq!(translated_messages.len(), 11);
        assert!(translated_messages.contains(&"Audio Codec".to_string()));
        assert!(translated_messages.contains(&"Audio Channels".to_string()));
    }

    #[test]
    fn dialog_plan_matches_audio_encoding_surface_size() {
        let plan = audio_encoding_dialog_plan();

        assert_eq!(plan.content_width, DIALOG_CONTENT_WIDTH);
        assert_eq!(plan.content_height, DIALOG_CONTENT_HEIGHT);
        assert_eq!(DIALOG_CONTENT_WIDTH, 560);
        assert_eq!(DIALOG_CONTENT_HEIGHT, 550);
    }

    #[test]
    fn card_specs_keep_illustrations_titles_and_descriptions() {
        let text =
            BigAudioEncodingDialogText::translated(|message| format!("translated:{message}"));
        let specs = audio_encoding_card_specs(&text, 6, 5);

        assert_eq!(specs.handling.illustration_name, HANDLING_ILLUSTRATION);
        assert_eq!(specs.handling.title, "translated:Audio Handling");
        assert_eq!(
            specs.handling.description,
            "translated:Choose what to do with the audio track. 'Copy' keeps the original. 'Re-encode' allows changing codec, bitrate and applying audio cleaning."
        );
        assert_eq!(specs.codec.illustration_name, CODEC_ILLUSTRATION);
        assert_eq!(specs.codec.title, "translated:Audio Codec");
        assert_eq!(specs.bitrate.illustration_name, BITRATE_ILLUSTRATION);
        assert_eq!(specs.bitrate.title, "translated:Audio Bitrate");
        assert_eq!(specs.channels.illustration_name, CHANNELS_ILLUSTRATION);
        assert_eq!(specs.channels.title, "translated:Audio Channels");
    }

    #[test]
    fn custom_entry_specs_follow_last_dropdown_option() {
        let text = BigAudioEncodingDialogText::translated(|message| message.to_string());
        let specs = audio_encoding_card_specs(&text, 6, 5);

        assert_eq!(specs.bitrate_custom_entry.label, "Custom:");
        assert_eq!(specs.bitrate_custom_entry.visible_index, 5);
        assert_eq!(specs.channels_custom_entry.label, "Custom:");
        assert_eq!(specs.channels_custom_entry.visible_index, 4);

        let empty_specs = audio_encoding_card_specs(&text, 0, 0);
        assert_eq!(empty_specs.bitrate_custom_entry.visible_index, 0);
        assert_eq!(empty_specs.channels_custom_entry.visible_index, 0);
    }

    #[test]
    fn reencode_selected_index_controls_encoding_card_sensitivity() {
        assert_eq!(REENCODE_HANDLING_SELECTED_INDEX, 1);
        assert!(!is_reencode_handling_selected(0));
        assert!(is_reencode_handling_selected(1));
        assert!(!is_reencode_handling_selected(2));
    }
}
