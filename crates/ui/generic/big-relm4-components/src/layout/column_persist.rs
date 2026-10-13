//! Column width persistence: the `title=width,…` encoding table views save.
//! Apps own where the encoded string is stored.

/// Encode visible column titles and fixed widths as `title=width,title=width`.
///
/// Empty titles are skipped because they are not stable persistence keys.
#[must_use]
pub fn encode_column_width_entries<'a>(
    columns: impl IntoIterator<Item = (&'a str, i32)>,
) -> Option<String> {
    let encoded: Vec<String> = columns
        .into_iter()
        .filter(|(title, _)| !title.is_empty())
        .map(|(title, width)| format!("{title}={width}"))
        .collect();

    if encoded.is_empty() {
        None
    } else {
        Some(encoded.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_titled_columns_in_order() {
        let encoded =
            encode_column_width_entries([("Name", 240), ("Artist", 180), ("Duration", 72)]);

        assert_eq!(encoded.as_deref(), Some("Name=240,Artist=180,Duration=72"));
    }

    #[test]
    fn skips_empty_titles() {
        let encoded = encode_column_width_entries([("", 80), ("Size", 96)]);

        assert_eq!(encoded.as_deref(), Some("Size=96"));
    }

    #[test]
    fn returns_none_without_stable_titles() {
        let encoded = encode_column_width_entries([("", 80), ("", 96)]);

        assert_eq!(encoded, None);
    }
}
