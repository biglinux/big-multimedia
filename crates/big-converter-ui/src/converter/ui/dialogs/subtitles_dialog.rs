//! Subtitle handling dialog adapter.

use adw::prelude::*;
use big_media_components::subtitle::handling_dialog::{
    BigSubtitleHandlingDialogControls, BigSubtitleHandlingDialogText, show_subtitle_handling_dialog,
};
use big_media_runtime::ui_labels;

use crate::converter::ui::sidebar::{self, Sidebar};
use crate::i18n::i18n;

/// Present the subtitle-handling dialog modally on `parent`, binding
/// the subtitle combo and "only-extract" switch to `sidebar`.
pub fn show(parent: &impl IsA<gtk::Widget>, sidebar: &Sidebar) {
    let subtitle = ui_labels::subtitle_labels();
    let subtitle_refs = sidebar::as_str_refs(&subtitle);
    show_subtitle_handling_dialog(
        parent,
        BigSubtitleHandlingDialogText::translated(i18n),
        BigSubtitleHandlingDialogControls {
            subtitle_labels: &subtitle_refs,
            subtitle_combo: &sidebar.subtitle_combo,
            only_extract_switch: &sidebar.only_extract_subs_switch,
        },
    );
}
