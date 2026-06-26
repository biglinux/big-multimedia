//! Sidebar widget bindings: profile radios → preset, action rows → dialogs,
//! summary subtitles tracking combos, and a fan-out of every settings-mutating
//! widget into a single `on_settings_changed` callback. Audio-handling row
//! prompts an AlertDialog when handling=Copy because re-encode is required for
//! audio effects.

use adw::prelude::*;

use big_media_runtime::ui_labels;

use crate::i18n::i18n;

use super::{CODEC_IDS, Callback, PresetProfile, QUALITY_IDS, Sidebar, fire, set_combo_by_id};

pub(super) fn install_bindings(sidebar: &Sidebar) {
    install_audio_mode_visibility(sidebar);
    install_profile_radios(sidebar);
    install_action_rows(sidebar);
    install_summary_bindings(sidebar);
    install_settings_changed_bindings(sidebar);
}

fn install_audio_mode_visibility(sidebar: &Sidebar) {
    let bitrate_combo = sidebar.audio_mode_bitrate_combo.clone();
    let channels_combo = sidebar.audio_mode_channels_combo.clone();
    sidebar
        .audio_mode_format_combo
        .connect_selected_notify(move |combo| {
            let is_copy = combo.selected() == 0;
            let has_bitrate = matches!(combo.selected(), 1 | 2 | 5 | 6);
            bitrate_combo.set_visible(!is_copy && has_bitrate);
            channels_combo.set_visible(!is_copy);
        });

    let output_combo = sidebar.audio_cut_out_combo.clone();
    sidebar
        .audio_cut_combo
        .connect_selected_notify(move |combo| {
            output_combo.set_visible(combo.selected() != 0);
        });
}

fn install_profile_radios(sidebar: &Sidebar) {
    {
        let cb = sidebar.on_profile_selected.clone();
        let video_codec_combo = sidebar.video_codec_combo.clone();
        sidebar.radio_copy.connect_toggled(move |radio| {
            if radio.is_active() {
                set_combo_by_id(&video_codec_combo, CODEC_IDS, "copy");
                fire(&cb, PresetProfile::FastCopy);
            }
        });
    }
    {
        let cb = sidebar.on_profile_selected.clone();
        let video_codec_combo = sidebar.video_codec_combo.clone();
        let video_quality_combo = sidebar.video_quality_combo.clone();
        sidebar.radio_universal.connect_toggled(move |radio| {
            if radio.is_active() {
                set_combo_by_id(&video_codec_combo, CODEC_IDS, "h264");
                set_combo_by_id(&video_quality_combo, QUALITY_IDS, "medium");
                fire(&cb, PresetProfile::Universal);
            }
        });
    }
    {
        let cb = sidebar.on_profile_selected.clone();
        let video_codec_combo = sidebar.video_codec_combo.clone();
        let video_quality_combo = sidebar.video_quality_combo.clone();
        sidebar.radio_smaller.connect_toggled(move |radio| {
            if radio.is_active() {
                set_combo_by_id(&video_codec_combo, CODEC_IDS, "h265");
                set_combo_by_id(&video_quality_combo, QUALITY_IDS, "medium");
                fire(&cb, PresetProfile::MoreEfficient);
            }
        });
    }
    {
        let cb = sidebar.on_profile_selected.clone();
        let video_codec_combo = sidebar.video_codec_combo.clone();
        let video_quality_combo = sidebar.video_quality_combo.clone();
        sidebar.radio_quality.connect_toggled(move |radio| {
            if radio.is_active() {
                set_combo_by_id(&video_codec_combo, CODEC_IDS, "av1");
                set_combo_by_id(&video_quality_combo, QUALITY_IDS, "medium");
                fire(&cb, PresetProfile::SmallerFile);
            }
        });
    }
}

fn install_action_rows(sidebar: &Sidebar) {
    {
        let cb = sidebar.on_show_encoding_dialog.clone();
        let radio_custom = sidebar.radio_custom.clone();
        sidebar.customize_row.connect_activated(move |_| {
            radio_custom.set_active(true);
            fire(&cb, ());
        });
    }
    {
        let cb = sidebar.on_show_audio_dialog.clone();
        sidebar.audio_encoding_row.connect_activated(move |_| {
            fire(&cb, ());
        });
    }
    {
        let cb = sidebar.on_show_noise_dialog.clone();
        let audio_handling_combo = sidebar.audio_handling_combo.clone();
        sidebar.audio_settings_row.connect_activated(move |row| {
            if audio_handling_combo.selected() == 1 {
                fire(&cb, ());
            } else {
                let handling_labels = ui_labels::audio_handling_labels();
                let current = handling_labels
                    .get(audio_handling_combo.selected() as usize)
                    .cloned()
                    .unwrap_or_else(|| i18n("Copy (no change)"));
                let dialog = crate::shared::ui_helpers::new_confirm_dialog(
                    &i18n("Enable Audio Re-encoding?"),
                    &format!(
                        "{} \"{}\". {}",
                        i18n("Audio is currently set to"),
                        current,
                        i18n("Audio settings require re-encoding mode. Switch to Re-encode?")
                    ),
                    "enable",
                    &i18n("Enable Re-encode"),
                    false,
                );
                let audio_handling_combo = audio_handling_combo.clone();
                let open_cb = cb.clone();
                dialog.connect_response(None, move |_dialog, response| {
                    if response == "enable" {
                        audio_handling_combo.set_selected(1);
                        fire(&open_cb, ());
                    }
                });
                dialog.present(Some(row));
            }
        });
    }
    if let Some(audio_settings_row_audio_mode) = sidebar.audio_settings_row_audio_mode.as_ref() {
        let cb = sidebar.on_show_noise_dialog.clone();
        audio_settings_row_audio_mode.connect_activated(move |_| {
            fire(&cb, ());
        });
    }
    {
        let cb = sidebar.on_show_subtitles_dialog.clone();
        sidebar.subtitles_row.connect_activated(move |_| {
            fire(&cb, ());
        });
    }
    {
        let cb = sidebar.on_show_extra_dialog.clone();
        sidebar.extra_row.connect_activated(move |_| {
            fire(&cb, ());
        });
    }
    {
        let cb = sidebar.on_output_dir_clicked.clone();
        sidebar.output_row.connect_activated(move |_| {
            fire(&cb, ());
        });
    }
    {
        let cb = sidebar.on_show_noise_dialog.clone();
        sidebar.nr_sidebar_row.connect_activated(move |_| {
            fire(&cb, ());
        });
    }
}

fn install_summary_bindings(sidebar: &Sidebar) {
    {
        let customize_row = sidebar.customize_row.clone();
        sidebar
            .output_format_combo
            .connect_selected_notify(move |combo| {
                let container_labels = ui_labels::container_labels();
                let format = container_labels
                    .get(combo.selected() as usize)
                    .cloned()
                    .unwrap_or_else(|| i18n("MP4"));
                customize_row.set_subtitle(&format);
            });
    }
    {
        // Signal arg for the receiver + WEAK cross-captures: a strong
        // self-capture is a combo⇄handler cycle, and the two handlers
        // capturing each other's combo strongly is a mutual cycle — either
        // way both combos turn immortal and pin the noise-reduction group
        // past window close.
        let audio_codec_combo = sidebar.audio_codec_combo.downgrade();
        let audio_encoding_row = sidebar.audio_encoding_row.downgrade();
        let nr_sidebar_group = sidebar.nr_sidebar_group.downgrade();
        sidebar
            .audio_handling_combo
            .connect_selected_notify(move |combo| {
                let (Some(codec), Some(row), Some(group)) = (
                    audio_codec_combo.upgrade(),
                    audio_encoding_row.upgrade(),
                    nr_sidebar_group.upgrade(),
                ) else {
                    return;
                };
                update_audio_encoding_subtitle(combo, &codec, &row, &group);
            });
    }
    {
        let audio_handling_combo = sidebar.audio_handling_combo.downgrade();
        let audio_encoding_row = sidebar.audio_encoding_row.downgrade();
        let nr_sidebar_group = sidebar.nr_sidebar_group.downgrade();
        sidebar
            .audio_codec_combo
            .connect_selected_notify(move |combo| {
                let (Some(handling), Some(row), Some(group)) = (
                    audio_handling_combo.upgrade(),
                    audio_encoding_row.upgrade(),
                    nr_sidebar_group.upgrade(),
                ) else {
                    return;
                };
                update_audio_encoding_subtitle(&handling, combo, &row, &group);
            });
    }
    {
        let subtitles_row = sidebar.subtitles_row.clone();
        sidebar
            .subtitle_combo
            .connect_selected_notify(move |combo| {
                let subtitle_labels = ui_labels::subtitle_labels();
                let label = subtitle_labels
                    .get(combo.selected() as usize)
                    .cloned()
                    .unwrap_or_else(|| i18n("Extract to File"));
                subtitles_row.set_subtitle(&label);
            });
    }
}

fn install_settings_changed_bindings(sidebar: &Sidebar) {
    let settings_changed = sidebar.on_settings_changed.clone();
    connect_combo_changed(&sidebar.video_codec_combo, &settings_changed);
    connect_combo_changed(&sidebar.video_quality_combo, &settings_changed);
    connect_combo_changed(&sidebar.preset_combo, &settings_changed);
    connect_combo_changed(&sidebar.resolution_combo, &settings_changed);
    connect_combo_changed(&sidebar.output_format_combo, &settings_changed);
    connect_combo_changed(&sidebar.gpu_combo, &settings_changed);
    connect_combo_changed(&sidebar.gpu_device_combo, &settings_changed);
    connect_combo_changed(&sidebar.audio_handling_combo, &settings_changed);
    connect_combo_changed(&sidebar.audio_codec_combo, &settings_changed);
    connect_combo_changed(&sidebar.audio_bitrate_combo, &settings_changed);
    connect_combo_changed(&sidebar.audio_channels_combo, &settings_changed);
    connect_combo_changed(&sidebar.subtitle_combo, &settings_changed);
    connect_combo_changed(&sidebar.audio_mode_format_combo, &settings_changed);
    connect_combo_changed(&sidebar.audio_mode_bitrate_combo, &settings_changed);
    connect_combo_changed(&sidebar.audio_mode_channels_combo, &settings_changed);
    connect_combo_changed(&sidebar.audio_cut_combo, &settings_changed);
    connect_combo_changed(&sidebar.audio_cut_out_combo, &settings_changed);

    connect_switch_row_changed(&sidebar.gpu_partial_switch, &settings_changed);
    connect_switch_row_changed(&sidebar.only_extract_subs_switch, &settings_changed);
    connect_switch_changed(&sidebar.nr_enabled_switch, &settings_changed);
    connect_switch_changed(&sidebar.gate_enabled_switch, &settings_changed);
    connect_switch_changed(&sidebar.hpf_enabled_switch, &settings_changed);
    connect_switch_changed(&sidebar.normalize_switch, &settings_changed);
    connect_switch_changed(&sidebar.compressor_enabled_switch, &settings_changed);
    connect_switch_changed(&sidebar.eq_enabled_switch, &settings_changed);

    connect_entry_changed(&sidebar.custom_resolution_entry, &settings_changed);
    connect_entry_changed(&sidebar.custom_bitrate_entry, &settings_changed);
    connect_entry_changed(&sidebar.custom_channels_entry, &settings_changed);
    connect_entry_changed(&sidebar.ffmpeg_options_entry, &settings_changed);
    connect_adjustment_changed(&sidebar.nr_strength_adj, &settings_changed);
    connect_dropdown_changed(&sidebar.nr_model_combo, &settings_changed);
    connect_adjustment_changed(&sidebar.nr_speech_adj, &settings_changed);
    connect_adjustment_changed(&sidebar.nr_lookahead_adj, &settings_changed);
    connect_adjustment_changed(&sidebar.nr_recovery_adj, &settings_changed);
    connect_adjustment_changed(&sidebar.gate_intensity_adj, &settings_changed);
    connect_adjustment_changed(&sidebar.hpf_freq_adj, &settings_changed);
    connect_adjustment_changed(&sidebar.compressor_intensity_adj, &settings_changed);
    connect_dropdown_changed(&sidebar.eq_preset_combo, &settings_changed);
    connect_adjustment_changed(&sidebar.audio_mode_volume_adjustment, &settings_changed);
    connect_adjustment_changed(&sidebar.audio_speed_adj, &settings_changed);
    for adjustment in &sidebar.eq_band_adjs {
        connect_adjustment_changed(adjustment, &settings_changed);
    }
}

fn update_audio_encoding_subtitle(
    audio_handling_combo: &adw::ComboRow,
    audio_codec_combo: &adw::ComboRow,
    audio_encoding_row: &adw::ActionRow,
    nr_sidebar_group: &adw::PreferencesGroup,
) {
    let idx = audio_handling_combo.selected() as usize;
    let is_reencode = idx == 1;
    let handling_labels = ui_labels::audio_handling_labels();
    let handling = handling_labels
        .get(idx)
        .cloned()
        .unwrap_or_else(|| i18n("Copy (no change)"));
    if is_reencode {
        let codec_labels = ui_labels::audio_codec_labels();
        let codec = codec_labels
            .get(audio_codec_combo.selected() as usize)
            .cloned()
            .unwrap_or_else(|| i18n("AAC"));
        audio_encoding_row.set_subtitle(&format!("{handling} · {codec}"));
    } else {
        audio_encoding_row.set_subtitle(&handling);
    }
    nr_sidebar_group.set_visible(is_reencode);
}

fn connect_combo_changed(combo: &adw::ComboRow, callback: &Callback<()>) {
    let cb = callback.clone();
    combo.connect_selected_notify(move |_| fire(&cb, ()));
}

fn connect_dropdown_changed(dropdown: &gtk::DropDown, callback: &Callback<()>) {
    let cb = callback.clone();
    dropdown.connect_selected_notify(move |_| fire(&cb, ()));
}

fn connect_switch_row_changed(switch_row: &adw::SwitchRow, callback: &Callback<()>) {
    let cb = callback.clone();
    switch_row.connect_active_notify(move |_| fire(&cb, ()));
}

fn connect_switch_changed(switch_: &gtk::Switch, callback: &Callback<()>) {
    let cb = callback.clone();
    switch_.connect_active_notify(move |_| fire(&cb, ()));
}

fn connect_entry_changed(entry: &gtk::Entry, callback: &Callback<()>) {
    let cb = callback.clone();
    entry.connect_changed(move |_| fire(&cb, ()));
}

fn connect_adjustment_changed(adjustment: &gtk::Adjustment, callback: &Callback<()>) {
    let cb = callback.clone();
    adjustment.connect_value_changed(move |_| fire(&cb, ()));
}
