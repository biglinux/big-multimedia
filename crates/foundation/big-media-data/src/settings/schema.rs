//! Which stored values the key catalog in `keys.rs` accepts. Unknown keys are
//! kept when they obey the line-oriented format; known numeric values must
//! parse and be finite.

use super::keys::{self, ValueType};

/// Whether `value` may be stored for `key` as it is.
pub(super) fn valid(key: &str, value: &str) -> bool {
    normalized(key, value) == Some(value)
}

/// The value stored for `key`, or `None` when it is invalid. A typed value may
/// carry spaces from a hand edit and is stored trimmed; text is kept as written.
pub(super) fn normalized<'a>(key: &str, value: &'a str) -> Option<&'a str> {
    if key.is_empty()
        || key.len() > 128
        || value.len() > 65536
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || value.chars().any(|c| c.is_control())
    {
        return None;
    }
    let typed = value.trim();
    match typed_value_valid(key, typed) {
        Some(valid) => valid.then_some(typed),
        None => Some(value),
    }
}

/// Whether `typed` fits `key`'s type, or `None` for a text or unknown key.
fn typed_value_valid(key: &str, typed: &str) -> Option<bool> {
    let entry = keys::entry(key)?;
    if !entry.choices.is_empty() {
        return Some(entry.choices.contains(&typed));
    }
    let valid = match entry.value_type {
        ValueType::Bool => matches!(typed, "true" | "false"),
        ValueType::Int => typed.parse::<i32>().is_ok(),
        ValueType::Double => typed.parse::<f64>().is_ok_and(f64::is_finite),
        ValueType::Text => return None,
    };
    Some(valid)
}
