//! Writing a playlist to a file: the save picker and the result alert both
//! players show.

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;
use gtk::glib;

use crate::i18n::i18n;
use crate::media::playlist_scan::M3U_EXTENSIONS;

/// Save picker titled `title` for one playlist, offered as `playlist.m3u8`
/// and filtered to M3U files.
pub fn playlist_save_picker(title: String) -> FilePicker {
    FilePicker::save_file(title)
        .initial_name("playlist.m3u8")
        .filter(&i18n("M3U8 Playlist"), M3U_EXTENSIONS)
}

/// Alert body for one playlist written to a file, or not.
#[must_use]
pub fn playlist_export_message(exported: bool) -> String {
    if exported {
        i18n("Playlist exported.")
    } else {
        i18n(
            "The playlist was not exported. Check storage or choose a new filename; existing files were not replaced.",
        )
    }
}

/// Show `body` in the playlist export alert over `parent`, unless that window
/// has closed.
pub fn show_playlist_export_result(parent: Option<&glib::WeakRef<gtk::Window>>, body: &str) {
    let Some(parent) = parent.and_then(glib::WeakRef::upgrade) else {
        return;
    };
    let dialog = big_gtk_kit::alert::Alert::builder()
        .heading(i18n("Playlist export"))
        .body(body)
        .build();
    dialog.add_response("close", &i18n("Close"));
    dialog.present(Some(&parent));
}
