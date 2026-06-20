//! Advanced preferences page assembler: composes playback / EOF / screenshot
//! / cache / screensaver / GPU / custom-mpv-options groups in display order.
//! Tab is labeled "General" since it's the first one users see.

mod common;
mod playback;
mod screenshot;
mod system;

use adw::prelude::*;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

use super::AdvancedWidgets;

use playback::{build_adv_eof_group, build_adv_playback_group};
use screenshot::build_screenshot_group;
use system::{
    build_adv_cache_group, build_adv_custom_group, build_adv_gpu_group, build_adv_screensaver_group,
};

pub(super) fn create_advanced_page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("General"))
        .icon_name("emblem-system-symbolic")
        .build();

    page.add(&build_adv_playback_group(settings, mpv));
    page.add(&build_adv_eof_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&build_screenshot_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&build_adv_cache_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&build_adv_screensaver_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&build_adv_gpu_group(settings, is_advanced, adv_widgets));
    page.add(&build_adv_custom_group(settings, is_advanced, adv_widgets));

    page
}
