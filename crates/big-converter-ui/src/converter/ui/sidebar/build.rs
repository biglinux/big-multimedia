//! Top-level sidebar builder — composes Conversion + Editing pages and
//! the hidden data-holder combos that dialogs sync to.

use adw::prelude::*;

use crate::converter::app::ConverterMode;
use crate::i18n::i18n;
use crate::shared::audio_effects::noise_model_labels;

use big_audio_effects::equalizer::EQ_PRESET_LABELS;
use big_media_runtime::ui_labels;

use super::combo::make_combo;
use super::conversion_page::build_conversion_page;
use super::editing_page::build_editing_page;
use super::{Sidebar, bindings, new_cb};

#[allow(clippy::too_many_lines)] // GTK sidebar builder — long by nature
pub(super) fn build_sidebar(mode: ConverterMode) -> Sidebar {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.set_width_request(320);

    let header = adw::HeaderBar::builder()
        .title_widget(&gtk::Label::new(Some(&if mode.is_audio() {
            i18n("Audio Converter")
        } else {
            i18n("Video Converter")
        })))
        .show_end_title_buttons(false)
        .css_classes(["flat"])
        .build();
    container.append(&header);

    let view_stack = adw::ViewStack::new();

    let conversion = build_conversion_page(mode);
    let editing = build_editing_page();

    view_stack.add_titled(
        &conversion.conv_scroll,
        Some("conversion"),
        &i18n("Conversion"),
    );
    view_stack.add_titled(&editing.edit_scroll, Some("editing"), &i18n("Editing"));
    container.append(&view_stack);

    // Bind translated labels: combos borrow refs only for the make_combo call,
    // so each owned Vec can drop right after.
    let codec_lbl = ui_labels::codec_labels();
    let video_codec_combo = make_combo(&i18n("Codec"), &super::as_str_refs(&codec_lbl));
    video_codec_combo.set_selected(1);
    let quality_lbl = ui_labels::quality_labels();
    let video_quality_combo = make_combo(&i18n("Quality"), &super::as_str_refs(&quality_lbl));
    video_quality_combo.set_selected(3);
    let preset_lbl = ui_labels::preset_labels();
    let preset_combo = make_combo(&i18n("Preset"), &super::as_str_refs(&preset_lbl));
    preset_combo.set_selected(3);
    let resolution_lbl = ui_labels::resolution_labels();
    let resolution_combo = make_combo(&i18n("Resolution"), &super::as_str_refs(&resolution_lbl));
    let container_lbl = ui_labels::container_labels();
    let output_format_combo = make_combo(&i18n("Format"), &super::as_str_refs(&container_lbl));
    let gpu_combo = adw::ComboRow::builder().title(&i18n("GPU")).build();
    let gpu_device_combo = adw::ComboRow::builder().title(&i18n("GPU Device")).build();
    let audio_handling_lbl = ui_labels::audio_handling_labels();
    let audio_handling_combo = make_combo(&i18n("Audio"), &super::as_str_refs(&audio_handling_lbl));
    let audio_codec_lbl = ui_labels::audio_codec_labels();
    let audio_codec_combo = make_combo(&i18n("Audio Codec"), &super::as_str_refs(&audio_codec_lbl));
    let audio_bitrate_lbl = ui_labels::audio_bitrate_labels();
    let audio_bitrate_combo = make_combo(&i18n("Bitrate"), &super::as_str_refs(&audio_bitrate_lbl));
    audio_bitrate_combo.set_selected(2);
    let audio_channels_lbl = ui_labels::audio_channels_labels();
    let audio_channels_combo =
        make_combo(&i18n("Channels"), &super::as_str_refs(&audio_channels_lbl));
    let subtitle_lbl = ui_labels::subtitle_labels();
    let subtitle_combo = make_combo(&i18n("Subtitles"), &super::as_str_refs(&subtitle_lbl));
    let gpu_partial_switch = adw::SwitchRow::builder()
        .title(&i18n("Decode Using Software"))
        .build();
    let only_extract_subs_switch = adw::SwitchRow::builder()
        .title(&i18n("Only Extract Subtitles"))
        .build();

    // Stage 9: although these hidden state-holders are never appended to the
    // widget tree (dialogs sync to them via property bindings), set an
    // accessible label so a tooling sweep that *does* pick them up — e.g.
    // a future debug overlay or Inspector dump — never reports an unnamed
    // control.
    let custom_resolution_entry = gtk::Entry::builder().placeholder_text("1280x720").build();
    custom_resolution_entry
        .update_property(&[gtk::accessible::Property::Label(&i18n("Custom resolution"))]);
    let custom_bitrate_entry = gtk::Entry::builder().placeholder_text("192k").build();
    custom_bitrate_entry
        .update_property(&[gtk::accessible::Property::Label(&i18n("Custom bitrate"))]);
    let custom_channels_entry = gtk::Entry::builder().placeholder_text("2").build();
    custom_channels_entry.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Custom audio channel count",
    ))]);
    let ffmpeg_options_entry = gtk::Entry::builder()
        .placeholder_text("-ss 60 -t 30")
        .build();
    ffmpeg_options_entry.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Extra ffmpeg options",
    ))]);

    let nr_enabled_switch = gtk::Switch::new();
    nr_enabled_switch.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Enable noise reduction",
    ))]);
    let nr_strength_adj = gtk::Adjustment::new(0.7, 0.0, 1.0, 0.05, 0.1, 0.0);
    let nr_model_labels = noise_model_labels();
    let nr_model_label_refs: Vec<&str> = nr_model_labels.iter().map(String::as_str).collect();
    let nr_model_combo = gtk::DropDown::from_strings(&nr_model_label_refs);
    nr_model_combo.set_selected(0);
    nr_model_combo.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Noise reduction model",
    ))]);
    let nr_speech_adj = gtk::Adjustment::new(0.5, 0.0, 1.0, 0.05, 0.1, 0.0);
    let nr_lookahead_adj = gtk::Adjustment::new(0.0, 0.0, 100.0, 1.0, 10.0, 0.0);
    let nr_recovery_adj = gtk::Adjustment::new(0.3, 0.0, 1.0, 0.05, 0.1, 0.0);
    let gate_enabled_switch = gtk::Switch::new();
    gate_enabled_switch
        .update_property(&[gtk::accessible::Property::Label(&i18n("Enable noise gate"))]);
    let gate_intensity_adj = gtk::Adjustment::new(50.0, 0.0, 100.0, 1.0, 10.0, 0.0);
    let hpf_enabled_switch = gtk::Switch::new();
    hpf_enabled_switch.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Enable high-pass filter",
    ))]);
    let hpf_freq_adj = gtk::Adjustment::new(80.0, 20.0, 300.0, 1.0, 10.0, 0.0);
    let normalize_switch = gtk::Switch::new();
    normalize_switch.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Normalize loudness",
    ))]);
    let compressor_enabled_switch = gtk::Switch::new();
    compressor_enabled_switch
        .update_property(&[gtk::accessible::Property::Label(&i18n("Enable compressor"))]);
    let compressor_intensity_adj = gtk::Adjustment::new(0.5, 0.0, 1.0, 0.05, 0.1, 0.0);
    let eq_enabled_switch = gtk::Switch::new();
    eq_enabled_switch
        .update_property(&[gtk::accessible::Property::Label(&i18n("Enable equalizer"))]);
    let eq_preset_combo = gtk::DropDown::from_strings(&EQ_PRESET_LABELS);
    let eq_band_adjs: Vec<gtk::Adjustment> = (0..10)
        .map(|_| gtk::Adjustment::new(0.0, -12.0, 12.0, 0.5, 1.0, 0.0))
        .collect();

    let sidebar = Sidebar {
        mode,
        container,
        view_stack,
        gpu_group: conversion.gpu_group,
        radio_copy: conversion.radio_copy,
        radio_universal: conversion.radio_universal,
        radio_smaller: conversion.radio_smaller,
        radio_quality: conversion.radio_quality,
        radio_custom: conversion.radio_custom,
        customize_row: conversion.customize_row,
        audio_encoding_row: conversion.audio_encoding_row,
        audio_settings_row: conversion.audio_settings_row,
        subtitles_row: conversion.subtitles_row,
        extra_row: conversion.extra_row,
        audio_settings_row_audio_mode: conversion.audio_settings_row_audio_mode,
        video_codec_combo,
        video_quality_combo,
        preset_combo,
        resolution_combo,
        output_format_combo,
        gpu_combo,
        gpu_device_combo,
        audio_handling_combo,
        audio_codec_combo,
        audio_bitrate_combo,
        audio_channels_combo,
        subtitle_combo,
        gpu_partial_switch,
        only_extract_subs_switch,
        custom_resolution_entry,
        custom_bitrate_entry,
        custom_channels_entry,
        ffmpeg_options_entry,
        nr_enabled_switch,
        nr_strength_adj,
        nr_model_combo,
        nr_speech_adj,
        nr_lookahead_adj,
        nr_recovery_adj,
        gate_enabled_switch,
        gate_intensity_adj,
        hpf_enabled_switch,
        hpf_freq_adj,
        normalize_switch,
        compressor_enabled_switch,
        compressor_intensity_adj,
        eq_enabled_switch,
        eq_preset_combo,
        eq_band_adjs,
        audio_fmt_combo: conversion.audio_fmt_combo,
        audio_br_combo: conversion.audio_br_combo,
        audio_ch_combo: conversion.audio_ch_combo,
        audio_cut_combo: conversion.audio_cut_combo,
        audio_cut_out_combo: conversion.audio_cut_out_combo,
        audio_vol_adj: conversion.audio_vol_adj,
        audio_speed_adj: conversion.audio_speed_adj,
        editing_box: editing.editing_box,
        brightness_scale: editing.brightness_scale,
        contrast_scale: editing.contrast_scale,
        saturation_scale: editing.saturation_scale,
        hue_scale: editing.hue_scale,
        rotate_ccw_btn: editing.rotate_ccw_btn,
        rotate_cw_btn: editing.rotate_cw_btn,
        flip_h_btn: editing.flip_h_btn,
        flip_v_btn: editing.flip_v_btn,
        reset_transform_btn: editing.reset_transform_btn,
        segments_listbox: editing.segments_listbox,
        segments_placeholder: editing.segments_placeholder,
        segments_list_row: editing.segments_list_row,
        add_segment_btn: editing.add_segment_btn,
        clear_segments_btn: editing.clear_segments_btn,
        output_mode_combo: editing.output_mode_combo,
        nr_sidebar_group: editing.nr_sidebar_group,
        nr_sidebar_row: editing.nr_sidebar_row,
        output_row: conversion.output_row,
        delete_switch: conversion.delete_switch,
        folder_combo: conversion.folder_combo,
        folder_entry: conversion.folder_entry,
        folder_btn: conversion.folder_btn,
        on_profile_selected: new_cb(),
        on_show_encoding_dialog: new_cb(),
        on_show_audio_dialog: new_cb(),
        on_show_noise_dialog: new_cb(),
        on_show_subtitles_dialog: new_cb(),
        on_show_extra_dialog: new_cb(),
        on_output_dir_clicked: new_cb(),
        on_settings_changed: new_cb(),
    };

    bindings::install_bindings(&sidebar);
    sidebar.update_audio_encoding_subtitle();
    sidebar
}
