// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Text / number formatting helpers shared across BigLinux GTK apps.

pub(crate) fn escape_markup_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\'' => escaped.push_str("&apos;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_markup_text_matches_glib_markup_entities() {
        assert_eq!(
            escape_markup_text("<b>\"A&B's\"</b>"),
            "&lt;b&gt;&quot;A&amp;B&apos;s&quot;&lt;/b&gt;"
        );
    }
}
