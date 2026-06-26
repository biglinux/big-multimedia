//! Video encoding dialog adapter.

use adw::prelude::*;
use big_media_components::video::encoding_dialog::{
    BigVideoEncodingDialogControls, BigVideoEncodingDialogText, show_video_encoding_dialog,
};
use big_media_runtime::ui_labels;

use crate::converter::ui::sidebar::{self, Sidebar};
use crate::i18n::i18n;

/// Present the video-encoding dialog modally on `parent`, binding
/// every container/codec/quality/resolution/preset control to
/// `sidebar`.
pub fn show(parent: &impl IsA<gtk::Widget>, sidebar: &Sidebar) {
    let container = ui_labels::container_labels();
    let codec = ui_labels::codec_labels();
    let quality = ui_labels::quality_labels();
    let resolution = ui_labels::resolution_labels();
    let preset = ui_labels::preset_labels();
    let container_refs = sidebar::as_str_refs(&container);
    let codec_refs = sidebar::as_str_refs(&codec);
    let quality_refs = sidebar::as_str_refs(&quality);
    let resolution_refs = sidebar::as_str_refs(&resolution);
    let preset_refs = sidebar::as_str_refs(&preset);
    show_video_encoding_dialog(
        parent,
        BigVideoEncodingDialogText::translated(i18n),
        BigVideoEncodingDialogControls {
            container_labels: &container_refs,
            container_combo: &sidebar.output_format_combo,
            codec_labels: &codec_refs,
            codec_combo: &sidebar.video_codec_combo,
            quality_labels: &quality_refs,
            quality_combo: &sidebar.video_quality_combo,
            resolution_labels: &resolution_refs,
            resolution_option_count: sidebar::RESOLUTION_IDS.len(),
            resolution_combo: &sidebar.resolution_combo,
            custom_resolution_entry: &sidebar.custom_resolution_entry,
            preset_labels: &preset_refs,
            preset_combo: &sidebar.preset_combo,
            software_decode_switch: &sidebar.gpu_partial_switch,
        },
    );
}
