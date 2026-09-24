//! `AdwAlertDialog` builders (content / error / confirm) with localized
//! standard button labels.
//! bigagents: app-local-adapter; common builders live in `big_app_kit::dialogs`.

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
