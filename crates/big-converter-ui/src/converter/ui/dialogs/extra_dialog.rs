//! Extra converter options dialog adapter.

use adw::prelude::*;
use big_media_components::converter::extra_dialog::{
    BigConverterExtraDialogControls, BigConverterExtraDialogText, show_converter_extra_dialog,
};

use crate::converter::ui::sidebar::Sidebar;
use crate::i18n::i18n;

/// Present the "Extra options" dialog modally on `parent`.
///
/// Binds the free-form FFmpeg options entry to `sidebar` and exposes
/// export/import/reset callbacks for the user's profile.
pub fn show(
    parent: &impl IsA<gtk::Widget>,
    sidebar: &Sidebar,
    on_export_profile: impl Fn() + 'static,
    on_import_profile: impl Fn() + 'static,
    _on_reset_settings: impl Fn() + 'static,
) {
    show_converter_extra_dialog(
        parent,
        BigConverterExtraDialogText::translated(i18n),
        BigConverterExtraDialogControls {
            ffmpeg_options_entry: &sidebar.ffmpeg_options_entry,
        },
        on_export_profile,
        on_import_profile,
    );
}
