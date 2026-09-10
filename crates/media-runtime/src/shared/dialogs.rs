//! `AdwAlertDialog` builders (content / error / confirm) with localized
//! standard button labels.
//! bigagents: app-local-adapter; common builders live in `big_app_kit::dialogs`.

/// Returns `(dialog, content_box)` so callers only need to populate content.
pub fn new_content_dialog(heading: &str) -> (adw::AlertDialog, gtk::Box) {
    big_app_kit::dialogs::content_dialog(heading, &crate::i18n::i18n("Close"))
}

/// Use for critical feedback — `AdwToast` is prohibited for errors (auto-dismisses).
pub fn new_error_dialog(heading: &str, body: &str) -> adw::AlertDialog {
    big_app_kit::dialogs::error_dialog(heading, body, &crate::i18n::i18n("Close"))
}

/// `confirm_id` = response id for the action button. Caller wires `connect_response`.
pub fn new_confirm_dialog(
    heading: &str,
    body: &str,
    confirm_id: &str,
    confirm_label: &str,
    destructive: bool,
) -> adw::AlertDialog {
    big_app_kit::dialogs::confirm_dialog(
        heading,
        body,
        &crate::i18n::i18n("Cancel"),
        confirm_id,
        confirm_label,
        destructive,
    )
}
