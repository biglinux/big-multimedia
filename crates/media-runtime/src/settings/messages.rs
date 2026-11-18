//! Extraction anchors for save/recovery states selected dynamically by the UI.
//! Keep these msgids aligned with the settings and library dialogs.
use crate::i18n::gettext_noop;

const _: &[&str] = &[
    gettext_noop("Access could not be verified — check permissions before reviewing"),
    gettext_noop("All preferences are saved."),
    gettext_noop("Available again"),
    gettext_noop("Cancellation requested. Finishing the current safe step…"),
    gettext_noop("Changes are saved."),
    gettext_noop("Changes were applied, but safe storage is not confirmed. Retry saving."),
    gettext_noop(
        "Changes work for this session but could not be saved. Check storage or close other instances, then retry.",
    ),
    gettext_noop("Not found — select only if you want to forget this library record"),
    gettext_noop("Remote items are not checked as local files"),
    gettext_noop("Saving your changes…"),
    gettext_noop("This location is not a regular media file"),
];
