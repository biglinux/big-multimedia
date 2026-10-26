#[cfg(not(miri))]
use gettextrs::{gettext, ngettext, pgettext};

#[must_use]
pub fn i18n(s: &str) -> String {
    #[cfg(miri)]
    {
        s.to_string()
    }
    #[cfg(not(miri))]
    {
        gettext(s)
    }
}

/// `msgid` within `context`, for an English word that means different things
/// in different places (the "Light" of picture quality is not a light color).
#[must_use]
pub fn pi18n(context: &str, msgid: &str) -> String {
    #[cfg(miri)]
    {
        let _ = context;
        msgid.to_string()
    }
    #[cfg(not(miri))]
    {
        pgettext(context, msgid)
    }
}

/// Marks a msgid literal for `xgettext` extraction (keyword `gettext_noop`)
/// without translating it, for strings defined as `&'static str` in a const
/// or struct field and translated later via `i18n(var)`. Without this the
/// literal never reaches the catalog and renders untranslated.
#[must_use]
pub const fn gettext_noop(msgid: &'static str) -> &'static str {
    msgid
}

/// `singular`/`plural` are English forms; locale picks one based on `n`. Result
/// keeps `{}` placeholders for the caller to fill in.
#[must_use]
pub fn ni18n(singular: &str, plural: &str, n: u64) -> String {
    #[cfg(miri)]
    {
        if n == 1 {
            singular.to_string()
        } else {
            plural.to_string()
        }
    }
    #[cfg(not(miri))]
    {
        ngettext(singular, plural, u32::try_from(n).unwrap_or(u32::MAX))
    }
}
