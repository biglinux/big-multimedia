// SPDX-License-Identifier: MIT

//! SQLite FTS5 query helpers.
//!
//! `fts_phrase` wraps a single user-supplied token as an FTS5 phrase. FTS5
//! phrase syntax treats `"` as a quote delimiter; doubling `"` inside a
//! phrase yields a literal `"`. Other FTS5 special characters
//! (`*`, `:`, `-`, `(`, `)`, `^`) are inert inside a phrase but become
//! syntactically meaningful outside, so phrase-wrapping every token is
//! the safe default.
//!
//! Use [`fts_match_query`] to build a multi-token MATCH expression with
//! prefix search appended to each phrase.

/// Wrap a user token as a quoted FTS5 phrase.
///
/// Doubles any internal `"` so the resulting string is parser-safe.
///
/// # Examples
///
/// ```no_run
/// use big_media_data::db::fts::fts_phrase;
/// assert_eq!(fts_phrase("ab"), "\"ab\"");
/// assert_eq!(fts_phrase("a\"b"), "\"a\"\"b\"");
/// assert_eq!(fts_phrase("DROP TABLE"), "\"DROP TABLE\"");
/// ```
#[must_use]
pub fn fts_phrase(token: &str) -> String {
    let mut out = String::with_capacity(token.len() + 2);
    out.push('"');
    for c in token.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Build an FTS5 `MATCH` query string from arbitrary user input.
///
/// - Splits the input on whitespace.
/// - Wraps each token as a quoted phrase via [`fts_phrase`].
/// - Appends `*` to each phrase for prefix search.
/// - Joins tokens with spaces (implicit AND in FTS5).
/// - Returns an empty string when the input is whitespace-only — the
///   caller must skip the filter in that case to avoid an FTS5 syntax
///   error.
///
/// # Examples
///
/// ```no_run
/// use big_media_data::db::fts::fts_match_query;
/// assert_eq!(fts_match_query(""), "");
/// assert_eq!(fts_match_query("   "), "");
/// assert_eq!(fts_match_query("foo bar"), "\"foo\"* \"bar\"*");
/// // Hostile input is neutralised by phrase-quoting.
/// assert_eq!(
///     fts_match_query("a\"b OR c"),
///     "\"a\"\"b\"* \"OR\"* \"c\"*"
/// );
/// ```
#[must_use]
pub fn fts_match_query(filter_text: &str) -> String {
    filter_text
        .split_whitespace()
        .map(|token| {
            let mut phrase = fts_phrase(token);
            phrase.push('*');
            phrase
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_empty_query() {
        assert_eq!(fts_match_query(""), "");
        assert_eq!(fts_match_query("   \t\n"), "");
    }

    #[test]
    fn single_token_gets_prefix_search() {
        assert_eq!(fts_match_query("song"), "\"song\"*");
    }

    #[test]
    fn multiple_tokens_join_with_space() {
        assert_eq!(fts_match_query("foo bar baz"), "\"foo\"* \"bar\"* \"baz\"*");
    }

    #[test]
    fn embedded_double_quote_is_doubled() {
        assert_eq!(fts_phrase(r#"a"b"#), r#""a""b""#);
    }

    #[test]
    fn fts5_operators_are_neutralised_inside_phrase() {
        // `OR`, `*`, `(` lose operator meaning when wrapped in a phrase.
        let q = fts_match_query("a OR (b*c)");
        assert!(q.contains(r#""OR"*"#));
        assert!(q.contains(r#""(b*c)"*"#));
    }

    #[test]
    fn sql_injection_payload_is_quoted_verbatim() {
        let payload = "'); DROP TABLE tracks; --";
        let q = fts_match_query(payload);
        // Every token is wrapped; nothing escapes the FTS5 phrase parser.
        assert!(q.contains(r#""'");"#) || q.contains(r#""');"#));
        assert!(q.contains(r#""tracks;"*"#));
    }
}
