//! Screenshot preferences group: format (PNG / JPEG / WebP), JPEG quality
//! (0-100) and PNG compression (0-9), filename template, save directory,
//! colorspace tagging, high bit depth. Quality / compression / template /
//! tagging / bit-depth are advanced-gated and self-register in `adv_widgets`.

use adw::prelude::*;
use big_relm4_components::layout::preferences::track_widget_visibility;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

use super::super::AdvancedWidgets;
use super::super::{add_combo_row, add_entry_row, add_spin_int_row, add_switch_row};
use super::common::illustration;

pub(super) fn build_screenshot_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let ss_group = adw::PreferencesGroup::builder()
        .title(i18n("Screenshots"))
        .description(i18n("Format and save location for video screenshots"))
        .build();

    let m = mpv.cloned();
    let format_row = add_combo_row(
        &ss_group,
        &i18n("Format"),
        &i18n("JPEG is smaller, PNG preserves all quality, WebP offers the best of both"),
        &["PNG", "JPEG", "WebP"],
        &["png", "jpg", "webp"],
        "screenshot-format",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_screenshot_format(v);
            }
        },
    );
    format_row.add_prefix(&illustration("screenshot.svg"));

    let m = mpv.cloned();
    let jpeg_row = add_spin_int_row(
        &ss_group,
        &i18n("JPEG Quality"),
        &i18n("0-100 (only for JPEG format)"),
        "screenshot-jpeg-quality",
        0.0,
        100.0,
        1.0,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_screenshot_jpeg_quality(v);
            }
        },
    );
    jpeg_row.add_prefix(&illustration("quality_slider.svg"));
    track_widget_visibility(&jpeg_row, is_advanced, adv_widgets);

    let m = mpv.cloned();
    let png_row = add_spin_int_row(
        &ss_group,
        &i18n("PNG Compression"),
        &i18n("0-9 (only for PNG format)"),
        "screenshot-png-compression",
        0.0,
        9.0,
        1.0,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_screenshot_png_compression(v);
            }
        },
    );
    png_row.add_prefix(&illustration("quality_slider.svg"));
    track_widget_visibility(&png_row, is_advanced, adv_widgets);

    let m = mpv.cloned();
    let template_row = add_entry_row(
        &ss_group,
        &i18n("Filename Template"),
        &i18n("%F=filename, %p=time, %n=number"),
        "screenshot-template",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_screenshot_template(v);
            }
        },
    );
    template_row.add_prefix(&illustration("file_template.svg"));
    track_widget_visibility(&template_row, is_advanced, adv_widgets);

    let m = mpv.cloned();
    let dir_row = add_entry_row(
        &ss_group,
        &i18n("Save Directory"),
        &i18n("Leave empty for default Pictures folder"),
        "screenshot-directory",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                if !v.is_empty() {
                    mpv.set_screenshot_directory(v);
                }
            }
        },
    );
    dir_row.add_prefix(&illustration("screenshot.svg"));

    let m = mpv.cloned();
    let tag_row = add_switch_row(
        &ss_group,
        &i18n("Tag Colorspace"),
        &i18n("Include color profile in screenshots"),
        "screenshot-tag-colorspace",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_screenshot_tag_colorspace(v);
            }
        },
    );
    tag_row.add_prefix(&illustration("color_tag.svg"));
    track_widget_visibility(&tag_row, is_advanced, adv_widgets);

    let m = mpv.cloned();
    let bitdepth_row = add_switch_row(
        &ss_group,
        &i18n("High Bit Depth"),
        &i18n("Use high bit depth for screenshots"),
        "screenshot-high-bit-depth",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_screenshot_high_bit_depth(v);
            }
        },
    );
    bitdepth_row.add_prefix(&illustration("color_tag.svg"));
    track_widget_visibility(&bitdepth_row, is_advanced, adv_widgets);

    ss_group
}
