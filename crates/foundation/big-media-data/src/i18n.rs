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

/// Gettext domain of the iso-codes package's country names.
#[cfg(not(miri))]
const ISO_3166_DOMAIN: &str = "iso_3166-1";

/// `country`, an English name from Radio Browser, in the person's language
/// through the iso-codes catalog when it is installed; otherwise as given.
#[must_use]
pub fn country_name(country: &str) -> String {
    for msgid in [country.to_owned(), iso_spelling(country)] {
        #[cfg(not(miri))]
        let translated = gettextrs::dgettext(ISO_3166_DOMAIN, msgid.as_str());
        #[cfg(miri)]
        let translated = msgid.clone();
        if translated != msgid {
            return translated;
        }
    }
    country.to_owned()
}

/// Radio Browser writes some names with a leading "The" and a capital "Of",
/// "And" or "The" inside, where iso-codes writes them in lower case.
fn iso_spelling(country: &str) -> String {
    country
        .strip_prefix("The ")
        .unwrap_or(country)
        .replace(" Of ", " of ")
        .replace(" And ", " and ")
        .replace(" The ", " the ")
}

#[cfg(test)]
mod tests {
    use super::iso_spelling;

    #[test]
    fn country_names_take_iso_spelling() {
        assert_eq!(
            iso_spelling("The United States Of America"),
            "United States of America"
        );
        assert_eq!(
            iso_spelling("The United Kingdom Of Great Britain And Northern Ireland"),
            "United Kingdom of Great Britain and Northern Ireland"
        );
        assert_eq!(iso_spelling("The Russian Federation"), "Russian Federation");
        assert_eq!(iso_spelling("Brazil"), "Brazil");
        assert_eq!(iso_spelling(""), "");
    }
}
