//! File information dialog adapter.

use std::path::Path;

use adw::prelude::*;
use big_media_components::media::file_info_dialog::{
    BigMediaFileInfoDialogLabels, BigMediaFileInfoDialogSpec, show_media_file_info_dialog,
};

use crate::converter::ffmpeg::progress;
use crate::i18n::i18n;
use crate::shared::ui_helpers::spawn_blocking_result;

/// Open the "File information" dialog over `parent` for the media file at
/// `file_path`. Duration and codec probing run two `ffprobe` child processes,
/// so they are offloaded to a worker thread; the dialog is presented once the
/// probes finish (the GTK main loop never blocks). No-op if `parent` is gone by
/// then. Wires "Open containing folder" to the desktop helper.
pub fn show_file_info_dialog(parent: &impl IsA<gtk::Window>, file_path: &Path) {
    let path = file_path.to_path_buf();
    let parent_weak = parent.upcast_ref::<gtk::Window>().downgrade();

    spawn_blocking_result(
        move || {
            let duration_secs = progress::probe_duration(&path);
            let media_info = progress::probe_media_metadata(&path);
            (path, duration_secs, media_info)
        },
        move |(path, duration_secs, media_info)| {
            let Some(parent) = parent_weak.upgrade() else {
                return;
            };
            let spec = BigMediaFileInfoDialogSpec::new(
                path,
                BigMediaFileInfoDialogLabels::translated(i18n),
            )
            .duration_secs(duration_secs)
            .media_info(media_info)
            .open_folder(|path| big_app_kit::desktop::open_containing_folder(&path));

            show_media_file_info_dialog(&parent, spec);
        },
    );
}
