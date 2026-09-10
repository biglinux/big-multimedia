//! Fuzzy string matching for search-as-you-type filtering.

/// Returns score (lower = better) when all `query` chars appear in order. `None` = no match.
#[must_use]
pub fn fuzzy_match(query: &str, text: &str) -> Option<u32> {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();
    let query_chars: Vec<char> = query_lower.chars().collect();
    let text_chars: Vec<char> = text_lower.chars().collect();

    if query_chars.is_empty() {
        return Some(0);
    }

    let mut qi = 0;
    let mut score: u32 = 0;
    let mut last_match_pos: i32 = -1;
    let mut consecutive_bonus = 0_u32;

    for (ti, &tc) in text_chars.iter().enumerate() {
        if qi < query_chars.len() && tc == query_chars[qi] {
            if ti as i32 == last_match_pos + 1 {
                consecutive_bonus += 1;
            } else {
                let gap = ti as i32 - last_match_pos;
                score += gap as u32;
            }
            last_match_pos = ti as i32;
            qi += 1;
        }
    }

    if qi == query_chars.len() {
        let start_bonus = if text_lower.starts_with(&query_lower) {
            100
        } else {
            0
        };
        Some(
            score
                .saturating_sub(consecutive_bonus * 2)
                .saturating_sub(start_bonus),
        )
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match_exact() {
        assert!(fuzzy_match("hello", "hello").is_some());
    }

    #[test]
    fn fuzzy_match_subsequence() {
        assert!(fuzzy_match("hlo", "hello").is_some());
    }

    #[test]
    fn fuzzy_match_no_match() {
        assert!(fuzzy_match("xyz", "hello").is_none());
    }

    #[test]
    fn fuzzy_match_empty_query() {
        assert_eq!(fuzzy_match("", "hello"), Some(0));
    }

    #[test]
    fn fuzzy_match_case_insensitive() {
        assert!(fuzzy_match("HELLO", "hello").is_some());
    }

    #[test]
    fn fuzzy_match_prefix_scores_better() {
        let prefix_score = fuzzy_match("he", "hello").unwrap();
        let middle_score = fuzzy_match("ll", "hello").unwrap();
        assert!(prefix_score <= middle_score);
    }
}
