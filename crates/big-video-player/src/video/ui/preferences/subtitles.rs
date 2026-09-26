//! Subtitles page: which subtitles load, how the text looks, where it sits,
//! sound descriptions, and picture-based (DVD/Blu-ray) subtitles.

use adw::prelude::*;
use gtk::pango;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};

use super::{
    add_color_row, add_combo_row, add_entry_row, add_spin_double_row, add_spin_int_row,
    add_switch_row, live, live_str,
};

pub(super) fn page(settings: &Settings, mpv: Option<&Arc<MpvBackend>>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Subtitles"))
        .icon_name("media-view-subtitles-symbolic")
        .build();

    let loading = adw::PreferencesGroup::builder()
        .title(i18n("Language and Files"))
        .description(i18n(
            "Use language codes separated by commas, in order of preference, such as pt,en",
        ))
        .build();
    add_entry_row(
        &loading,
        &i18n("Preferred Languages"),
        "",
        "preferred-sub-lang",
        settings,
        live_str(mpv, MpvBackend::set_preferred_sub_lang),
    );
    add_combo_row(
        &loading,
        &i18n("Load Subtitle Files"),
        &i18n("Subtitle files found in the same folder as the video"),
        &[
            &i18n("Similar name"),
            &i18n("Exact name"),
            &i18n("All in the folder"),
            &i18n("Never"),
        ],
        &["fuzzy", "exact", "all", "no"],
        "sub-auto",
        settings,
        live_str(mpv, MpvBackend::set_sub_auto),
    );
    page.add(&loading);

    let text = adw::PreferencesGroup::builder().title(i18n("Text")).build();
    text.add(&font_row(settings, mpv));
    add_spin_int_row(
        &text,
        &i18n("Size"),
        "",
        "subtitle-font-size",
        10.0,
        100.0,
        1.0,
        settings,
        live(mpv, MpvBackend::set_sub_font_size),
    );
    add_switch_row(
        &text,
        &i18n("Bold"),
        &i18n("Thicker letters are easier to read"),
        "sub-bold",
        settings,
        live(mpv, MpvBackend::set_sub_bold),
    );
    add_color_row(
        &text,
        &i18n("Color"),
        "",
        "sub-color",
        settings,
        live_str(mpv, MpvBackend::set_sub_color),
    );
    page.add(&text);

    let outline = adw::PreferencesGroup::builder()
        .title(i18n("Outline and Shadow"))
        .description(i18n("An outline keeps text readable over bright scenes"))
        .build();
    add_combo_row(
        &outline,
        &i18n("Style"),
        "",
        &[
            &i18n("Outline and shadow"),
            &i18n("Solid box"),
            &i18n("Box behind the text"),
        ],
        &["outline-and-shadow", "opaque-box", "background-box"],
        "sub-border-style",
        settings,
        live_str(mpv, MpvBackend::set_sub_border_style),
    );
    add_spin_double_row(
        &outline,
        &i18n("Outline Thickness"),
        "",
        "sub-border-size",
        0.0,
        10.0,
        0.5,
        1,
        settings,
        live(mpv, MpvBackend::set_sub_border_size),
    );
    add_color_row(
        &outline,
        &i18n("Outline Color"),
        "",
        "sub-border-color",
        settings,
        live_str(mpv, MpvBackend::set_sub_border_color),
    );
    add_spin_double_row(
        &outline,
        &i18n("Shadow Distance"),
        "",
        "sub-shadow-offset",
        0.0,
        10.0,
        0.5,
        1,
        settings,
        live(mpv, MpvBackend::set_sub_shadow_offset),
    );
    add_color_row(
        &outline,
        &i18n("Shadow Color"),
        &i18n("Lower the opacity for a softer shadow"),
        "sub-shadow-color",
        settings,
        live_str(mpv, MpvBackend::set_sub_shadow_color),
    );
    page.add(&outline);

    let position = adw::PreferencesGroup::builder()
        .title(i18n("Position and Size"))
        .build();
    add_spin_int_row(
        &position,
        &i18n("Distance From the Bottom"),
        &i18n("In pixels"),
        "sub-margin-y",
        0.0,
        200.0,
        1.0,
        settings,
        live(mpv, MpvBackend::set_sub_margin_y),
    );
    add_switch_row(
        &position,
        &i18n("Grow With the Window"),
        &i18n("Larger windows show larger subtitles"),
        "sub-scale-with-window",
        settings,
        live(mpv, MpvBackend::set_sub_scale_with_window),
    );
    add_switch_row(
        &position,
        &i18n("Grow Styled Subtitles Too"),
        &i18n("Also resize styled ASS/SSA subtitles, common in anime"),
        "sub-ass-scale-with-window",
        settings,
        live(mpv, MpvBackend::set_sub_ass_scale_with_window),
    );
    page.add(&position);

    let descriptions = adw::PreferencesGroup::builder()
        .title(i18n("Sound Descriptions"))
        .description(i18n(
            "Subtitles for the hearing impaired describe sounds, like [music] or (applause)",
        ))
        .build();
    let sdh_row = add_switch_row(
        &descriptions,
        &i18n("Hide Sound Descriptions"),
        "",
        "sub-filter-sdh",
        settings,
        live(mpv, MpvBackend::set_sub_filter_sdh),
    );
    let strict_row = add_switch_row(
        &descriptions,
        &i18n("Stricter Filtering"),
        &i18n("Removes more descriptions, but may also remove some dialogue"),
        "sub-filter-sdh-harder",
        settings,
        live(mpv, MpvBackend::set_sub_filter_sdh_harder),
    );
    sdh_row
        .bind_property("active", &strict_row, "sensitive")
        .sync_create()
        .build();
    page.add(&descriptions);

    let pictures = adw::PreferencesGroup::builder()
        .title(i18n("Picture Subtitles"))
        .description(i18n("DVD and Blu-ray subtitles are images instead of text"))
        .build();
    add_spin_double_row(
        &pictures,
        &i18n("Smooth Edges"),
        &i18n("Blurs pixelated letters slightly; 0 turns it off"),
        "sub-gauss",
        0.0,
        3.0,
        0.1,
        1,
        settings,
        live(mpv, MpvBackend::set_sub_gauss),
    );
    add_switch_row(
        &pictures,
        &i18n("Show in Gray"),
        &i18n("Replaces colored subtitles with gray ones"),
        "sub-gray",
        settings,
        live(mpv, MpvBackend::set_sub_gray),
    );
    page.add(&pictures);

    page
}

/// Typeface row. Only the family is stored, so the chooser only offers families.
fn font_row(settings: &Settings, mpv: Option<&Arc<MpvBackend>>) -> adw::ActionRow {
    let button = gtk::FontDialogButton::new(Some(gtk::FontDialog::new()));
    button.set_level(gtk::FontLevel::Family);
    button.set_valign(gtk::Align::Center);
    let current = settings.get_string(keys::SUB_FONT);
    if !current.is_empty() {
        button.set_font_desc(&pango::FontDescription::from_string(&current));
    }
    let settings = settings.clone();
    let mpv = mpv.cloned();
    button.connect_font_desc_notify(move |button| {
        let family = button
            .font_desc()
            .and_then(|description| description.family())
            .map(|family| family.to_string())
            .unwrap_or_default();
        let _ = settings.put_string(keys::SUB_FONT, &family);
        if let Some(mpv) = &mpv {
            mpv.set_sub_font(&family);
        }
    });
    let row = adw::ActionRow::builder().title(i18n("Font")).build();
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    row
}
