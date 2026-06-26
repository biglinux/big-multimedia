//! Audio encoding dialog adapter.

use adw::prelude::*;
use big_media_components::audio::encoding_dialog::{
    BigAudioEncodingDialogControls, BigAudioEncodingDialogText, show_audio_encoding_dialog,
};
use big_media_runtime::ui_labels;

use crate::converter::ui::sidebar::{self, Sidebar};
use crate::i18n::i18n;

/// Present the audio-encoding dialog modally on `parent`, binding
/// every control bidirectionally to the matching widget in `sidebar`.
pub fn show(parent: &impl IsA<gtk::Widget>, sidebar: &Sidebar) {
    // Owned String vecs must outlive the dialog call.
    let handling = ui_labels::audio_handling_labels();
    let codec = ui_labels::audio_codec_labels();
    let bitrate = ui_labels::audio_bitrate_labels();
    let channels = ui_labels::audio_channels_labels();
    let handling_refs = sidebar::as_str_refs(&handling);
    let codec_refs = sidebar::as_str_refs(&codec);
    let bitrate_refs = sidebar::as_str_refs(&bitrate);
    let channels_refs = sidebar::as_str_refs(&channels);
    show_audio_encoding_dialog(
        parent,
        BigAudioEncodingDialogText::translated(i18n),
        BigAudioEncodingDialogControls {
            handling_labels: &handling_refs,
            handling_combo: &sidebar.audio_handling_combo,
            codec_labels: &codec_refs,
            codec_combo: &sidebar.audio_codec_combo,
            bitrate_labels: &bitrate_refs,
            bitrate_option_count: sidebar::AUDIO_BITRATE_IDS.len(),
            bitrate_combo: &sidebar.audio_bitrate_combo,
            custom_bitrate_entry: &sidebar.custom_bitrate_entry,
            channels_labels: &channels_refs,
            channels_option_count: sidebar::AUDIO_CHANNELS_IDS.len(),
            channels_combo: &sidebar.audio_channels_combo,
            custom_channels_entry: &sidebar.custom_channels_entry,
        },
    );
}
