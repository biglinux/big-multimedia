use adw::prelude::*;
use big_app_kit::dialogs;

use super::Settings;

/// Restore saved dialog size and connect close handler to persist it.
pub fn bind_dialog_size_persistence(
    dialog: &adw::Window,
    settings: &Settings,
    key: &str,
    default_w: i32,
    default_h: i32,
) {
    let width = settings.int(&format!("dlg-{key}-w"));
    let height = settings.int(&format!("dlg-{key}-h"));
    dialog.set_default_size(
        if width > 0 { width } else { default_w },
        if height > 0 { height } else { default_h },
    );

    let settings = settings.clone();
    let key = key.to_string();
    dialog.connect_close_request(move |dlg| {
        dialogs::persist_dialog_size(dlg.width(), dlg.height(), |width, height| {
            let _ = settings.set_int(&format!("dlg-{key}-w"), width);
            let _ = settings.set_int(&format!("dlg-{key}-h"), height);
        });
        glib::Propagation::Proceed
    });
}
