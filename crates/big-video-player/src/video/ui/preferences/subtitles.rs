//! Subtitle preferences page assembler — composes the 5 groups built in
//! `subtitles_groups.rs` (font / colors / language / position / special
//! formats). Per-row advanced gating happens inside each builder.

use adw::prelude::*;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

use super::AdvancedWidgets;
use super::subtitles_groups::{
    build_colors_outline_group, build_font_group, build_language_loading_group,
    build_position_rendering_group, build_special_formats_group,
};

pub(super) fn create_subtitle_page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Subtitles"))
        .icon_name("media-view-subtitles-symbolic")
        .build();

    page.add(&build_font_group(settings, mpv, is_advanced, adv_widgets));
    page.add(&build_colors_outline_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&build_language_loading_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&build_position_rendering_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&build_special_formats_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));

    page
}
