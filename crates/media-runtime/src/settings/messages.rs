//! Extraction anchors for save/recovery states selected dynamically by the UI.
//! Keep these msgids aligned with settings/account/library dialogs. This module
//! performs no I/O and does not turn errors from external tools into UI text.
use crate::i18n::gettext_noop;

const _: &[&str] = &[
    gettext_noop(
        "A save is pending. Re-enter exactly the intended credentials to retry that same transaction, or close and use Recover account. A different payload will be refused.",
    ),
    gettext_noop("API key"),
    gettext_noop("API secret"),
    gettext_noop("Access could not be verified — check permissions before reviewing"),
    gettext_noop("All preferences are saved."),
    gettext_noop("An account operation was interrupted. Recover it before making another change."),
    gettext_noop("Available again"),
    gettext_noop("Cancellation requested. Finishing the current safe step…"),
    gettext_noop("Changes are saved."),
    gettext_noop("Changes were applied, but safe storage is not confirmed. Retry saving."),
    gettext_noop(
        "Changes work for this session but could not be saved. Check storage or close other instances, then retry.",
    ),
    gettext_noop("Check keyring access"),
    gettext_noop("Credentials are protected in your keyring."),
    gettext_noop(
        "Credentials are protected, but cleanup needs recovery. Close this editor and use Recover account.",
    ),
    gettext_noop("Credentials are still in a private file. Protect them in your desktop keyring."),
    gettext_noop("Credentials are stored in the desktop keyring."),
    gettext_noop("Edit the credentials, then save. Closing without saving discards your edits."),
    gettext_noop("No credentials are saved for this account."),
    gettext_noop("Not found — select only if you want to forget this library record"),
    gettext_noop("Playlist exported."),
    gettext_noop("Remote items are not checked as local files"),
    gettext_noop(
        "Saving was not fully confirmed. Close this editor, reload and recover the pending operation. No plaintext fallback was used.",
    ),
    gettext_noop("Saving your changes…"),
    gettext_noop("Session key"),
    gettext_noop(
        "The playlist was not exported. Check storage or choose a new filename; existing files were not replaced.",
    ),
    gettext_noop("This location is not a regular media file"),
    gettext_noop("User token"),
];
