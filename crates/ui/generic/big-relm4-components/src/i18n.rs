// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Component-owned user-visible strings (shared preference rows, page titles).
//!
//! Since the split these live in the ONE shared `big-media-runtime` textdomain
//! along with every other UI string, so a single catalog translates the whole
//! suite. The host app binds that domain once at boot
//! (`shared::app_base::init_gettext`); [`t`] resolves against it with an
//! explicit `dgettext`, so it never touches the process default domain or
//! locale and stays safe in embedded/multicall hosts. With no app binding
//! (standalone component demos/tests) it returns the msgid unchanged.

const DOMAIN: &str = "big-media-runtime";

/// Translate a component-owned msgid through the shared `big-media-runtime`
/// domain. Returns the msgid itself when no catalog is bound.
#[must_use]
pub fn t(msgid: &str) -> String {
    gettextrs::dgettext(DOMAIN, msgid)
}

/// Marks a component-owned string for extraction without translating.
#[must_use]
pub const fn gettext_noop(msgid: &'static str) -> &'static str {
    msgid
}
