//! Sidebar conversion page: profile preset radios (Copy/Universal/Smaller/
//! Quality/Custom), GPU combo, action rows for encoding/audio/subtitles/
//! extra, output-directory row + folder combo. Audio mode swaps the layout
//! to format/bitrate/channels + cut + audio-settings.

use adw::prelude::*;
use big_relm4_components::list::check_action_row::{
    BigCheckActionRow, BigCheckActionRowSpec, BigCheckRowActivation,
};
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};

use big_media_runtime::ui_labels;

use crate::converter::app::ConverterMode;
use crate::i18n::i18n;

use super::combo::make_combo;

pub(super) struct ConversionPageWidgets {
    pub(super) conv_scroll: gtk::ScrolledWindow,
    pub(super) gpu_group: adw::PreferencesGroup,
    pub(super) radio_copy: gtk::CheckButton,
    pub(super) radio_universal: gtk::CheckButton,
    pub(super) radio_smaller: gtk::CheckButton,
    pub(super) radio_quality: gtk::CheckButton,
    pub(super) radio_custom: gtk::CheckButton,
    pub(super) customize_row: adw::ActionRow,
    pub(super) audio_encoding_row: adw::ActionRow,
    pub(super) audio_settings_row: adw::ActionRow,
    pub(super) subtitles_row: adw::ActionRow,
    pub(super) extra_row: adw::ActionRow,
    pub(super) audio_settings_row_audio_mode: Option<adw::ActionRow>,
    pub(super) output_row: adw::ActionRow,
    pub(super) folder_combo: gtk::DropDown,
    pub(super) delete_switch: gtk::Switch,
    pub(super) folder_entry: gtk::Entry,
    pub(super) folder_btn: gtk::Button,
    pub(super) audio_fmt_combo: adw::ComboRow,
    pub(super) audio_br_combo: adw::ComboRow,
    pub(super) audio_ch_combo: adw::ComboRow,
    pub(super) audio_cut_combo: adw::ComboRow,
    pub(super) audio_cut_out_combo: adw::ComboRow,
    pub(super) audio_vol_adj: gtk::Adjustment,
    pub(super) audio_speed_adj: gtk::Adjustment,
}

#[allow(clippy::too_many_lines)] // GTK conversion-page builder — long by nature
pub(super) fn build_conversion_page(mode: ConverterMode) -> ConversionPageWidgets {
    let conv_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let conv_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    conv_box.set_margin_top(12);
    conv_box.set_margin_bottom(12);
    conv_box.set_margin_start(12);
    conv_box.set_margin_end(12);

    let audio_fmt_lbl = ui_labels::audio_format_labels();
    let audio_fmt_combo = make_combo(&i18n("Format"), &super::as_str_refs(&audio_fmt_lbl));
    audio_fmt_combo.set_selected(1);
    let audio_br_lbl = ui_labels::audio_bitrate_labels_audio_mode();
    let audio_br_combo = make_combo(&i18n("Bitrate"), &super::as_str_refs(&audio_br_lbl));
    audio_br_combo.set_selected(2);
    let audio_ch_lbl = ui_labels::audio_channels_labels_audio_mode();
    let audio_ch_combo = make_combo(&i18n("Channels"), &super::as_str_refs(&audio_ch_lbl));
    let audio_cut_combo = make_combo(
        &i18n("Mode"),
        &[
            i18n("Off").as_str(),
            i18n("Chronological").as_str(),
            i18n("Segment Number").as_str(),
        ],
    );
    audio_cut_combo.set_selected(0);
    audio_cut_combo.set_subtitle(&i18n(
        "Chronological exports by timeline order; Segment Number preserves marking order",
    ));
    let audio_cut_out_combo = make_combo(
        &i18n("Output"),
        &[
            i18n("Separate Files").as_str(),
            i18n("Merge into One").as_str(),
        ],
    );
    audio_cut_out_combo.set_subtitle(&i18n(
        "Separate Files creates one file per segment; Merge joins all",
    ));
    audio_cut_out_combo.set_visible(audio_cut_combo.selected() != 0);
    let audio_vol_adj = gtk::Adjustment::new(1.0, 0.0, 2.0, 0.05, 0.1, 0.0);
    let audio_speed_adj = gtk::Adjustment::new(1.0, 0.1, 5.0, 0.05, 0.1, 0.0);

    let profile_group = adw::PreferencesGroup::new();
    let copy = BigCheckActionRow::new(
        BigCheckActionRowSpec::new(i18n("Fast Copy")).subtitle(i18n("No re-encoding, fastest")),
        None,
    );
    let radio_copy = copy.check().clone();
    profile_group.add(copy.root());

    let universal = BigCheckActionRow::new(
        BigCheckActionRowSpec::new(i18n("Universal"))
            .subtitle(i18n("H.264, compatible with all devices"))
            .badge(i18n("Recommended"))
            .active(),
        Some(&radio_copy),
    );
    let radio_universal = universal.check().clone();
    profile_group.add(universal.root());

    let smaller = BigCheckActionRow::new(
        BigCheckActionRowSpec::new(i18n("More Efficient"))
            .subtitle(i18n("H.265, better compression")),
        Some(&radio_copy),
    );
    let radio_smaller = smaller.check().clone();
    profile_group.add(smaller.root());

    let quality = BigCheckActionRow::new(
        BigCheckActionRowSpec::new(i18n("Smaller File")).subtitle(i18n("AV1, best compression")),
        Some(&radio_copy),
    );
    let radio_quality = quality.check().clone();
    profile_group.add(quality.root());

    let custom = BigCheckActionRow::new(
        BigCheckActionRowSpec::new(i18n("Customize encoding..."))
            .subtitle(i18n("MP4"))
            .trailing_icon_name("go-next-symbolic")
            .activation(BigCheckRowActivation::OpenDetail),
        Some(&radio_copy),
    );
    let customize_row = custom.root().clone();
    let radio_custom = custom.check().clone();
    profile_group.add(&customize_row);

    let gpu_group = adw::PreferencesGroup::new();
    gpu_group.set_visible(false);
    let gpu_combo = adw::ComboRow::builder().title(&i18n("GPU")).build();
    gpu_combo.add_prefix(&gtk::Image::from_icon_name("video-display-symbolic"));
    gpu_group.add(&gpu_combo);

    let actions_group = adw::PreferencesGroup::new();
    let audio_handling_default = ui_labels::audio_handling_labels()
        .into_iter()
        .next()
        .unwrap_or_else(|| i18n("Copy (no change)"));
    let audio_encoding_row = BigNavigationRow::new(
        BigNavigationRowSpec::new(i18n("Audio Encoding"))
            .subtitle(audio_handling_default)
            .prefix_icon_name("audio-x-generic-symbolic"),
    )
    .into_root();
    actions_group.add(&audio_encoding_row);

    let audio_settings_row = BigNavigationRow::new(
        BigNavigationRowSpec::new(i18n("Audio Settings"))
            .subtitle(i18n("AI noise removal, gate, normalize"))
            .prefix_icon_name("audio-volume-high-symbolic"),
    )
    .into_root();
    actions_group.add(&audio_settings_row);

    let subtitles_row = BigNavigationRow::new(
        BigNavigationRowSpec::new(i18n("Subtitles"))
            .subtitle(i18n("Keep inside the video"))
            .prefix_icon_name("insert-text-symbolic"),
    )
    .into_root();
    actions_group.add(&subtitles_row);

    let extra_row = BigNavigationRow::new(
        BigNavigationRowSpec::new(i18n("Extra"))
            .subtitle(i18n("No customizations"))
            .prefix_icon_name("preferences-system-symbolic"),
    )
    .into_root();
    actions_group.add(&extra_row);

    let audio_settings_row_audio_mode = if mode.is_audio() {
        let output_group = adw::PreferencesGroup::new();
        output_group.set_margin_top(0);
        output_group.add(&audio_fmt_combo);
        output_group.add(&audio_br_combo);
        output_group.add(&audio_ch_combo);
        conv_box.append(&output_group);

        let cut_group = adw::PreferencesGroup::builder().title(i18n("Cut")).build();
        cut_group.set_margin_top(6);
        cut_group.add(&audio_cut_combo);
        cut_group.add(&audio_cut_out_combo);
        conv_box.append(&cut_group);

        let audio_settings_group = adw::PreferencesGroup::builder()
            .title(i18n("Audio Settings"))
            .build();
        audio_settings_group.set_margin_top(6);
        let audio_settings_row_audio_mode = BigNavigationRow::new(
            BigNavigationRowSpec::new(i18n("Audio Settings"))
                .subtitle(i18n("AI noise removal, gate, normalize"))
                .prefix_icon_name("audio-x-generic-symbolic"),
        )
        .into_root();
        audio_settings_group.add(&audio_settings_row_audio_mode);
        conv_box.append(&audio_settings_group);
        Some(audio_settings_row_audio_mode)
    } else {
        conv_box.append(&gpu_group);
        conv_box.append(&profile_group);
        conv_box.append(&actions_group);
        None
    };

    let output_dir = if mode.is_audio() {
        big_os_kit::xdg_dirs::audio_dir()
    } else {
        big_os_kit::xdg_dirs::video_dir()
    }
    .unwrap_or_default()
    .to_string_lossy()
    .to_string();
    let output_row = BigNavigationRow::new(
        BigNavigationRowSpec::new(i18n("Output Directory")).subtitle(output_dir),
    )
    .into_root();

    let folder_combo = gtk::DropDown::from_strings(&[
        &i18n("Save in the same folder as the original file"),
        &i18n("Folder to save"),
    ]);
    folder_combo.update_property(&[gtk::accessible::Property::Label(&i18n("Output folder"))]);
    let delete_switch = gtk::Switch::new();
    let folder_entry = gtk::Entry::builder()
        .placeholder_text(&i18n("Select folder"))
        .hexpand(true)
        .build();
    folder_entry.update_property(&[gtk::accessible::Property::Label(&i18n("Select folder"))]);
    let folder_btn = gtk::Button::builder().icon_name("folder-symbolic").build();
    folder_btn.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Choose output folder",
    ))]);

    conv_scroll.set_child(Some(&conv_box));

    ConversionPageWidgets {
        conv_scroll,
        gpu_group,
        radio_copy,
        radio_universal,
        radio_smaller,
        radio_quality,
        radio_custom,
        customize_row,
        audio_encoding_row,
        audio_settings_row,
        subtitles_row,
        extra_row,
        audio_settings_row_audio_mode,
        output_row,
        folder_combo,
        delete_switch,
        folder_entry,
        folder_btn,
        audio_fmt_combo,
        audio_br_combo,
        audio_ch_combo,
        audio_cut_combo,
        audio_cut_out_combo,
        audio_vol_adj,
        audio_speed_adj,
    }
}
