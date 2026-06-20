//! PII redaction for log / tracing output.
//!
//! Wraps user-data-bearing fields in a `Display` adapter that masks all but
//! the first and last characters. Use `redact_path()` for filesystem paths
//! (keeps the leaf filename ext but hides parent directories), and
//! `Redacted<T>` for arbitrary `Display` values (URLs, hostnames, tokens).
//!
//! Local module — `Redacted<T>` is broadly useful and a sibling helper may
//! later land in `big-app-kit::redaction` (constitution allows app-local
//! adapters when no cross-app crate hosts it yet).

use std::fmt;
use std::path::Path;

/// Display adapter that masks middle characters of any `Display` value.
///
/// Mask rule: keep first + last char, replace rest with `*`. Strings of len
/// <= 2 collapse to `**` to avoid leaking 1-char secrets.
#[allow(dead_code)] // Public PII helper; not all sites use it yet.
pub struct Redacted<T: fmt::Display>(pub T);

impl<T: fmt::Display> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // alloc once; PII sites are not perf hot paths
        let s = self.0.to_string();
        let chars: Vec<char> = s.chars().collect();
        match chars.len() {
            0 => f.write_str("<empty>"),
            1 | 2 => f.write_str("**"),
            n => {
                f.write_str(&chars[0].to_string())?;
                for _ in 1..n - 1 {
                    f.write_str("*")?;
                }
                f.write_str(&chars[n - 1].to_string())
            }
        }
    }
}

impl<T: fmt::Display> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Redact a filesystem path: keep only the file-name component, hide the
/// parent directory tree. Leaks ext but not user-identifying segments.
pub fn redact_path(p: impl AsRef<Path>) -> String {
    let p = p.as_ref();
    p.file_name().map_or_else(
        || "<redacted-path>".to_string(),
        |n| format!("<redacted>/{}", n.to_string_lossy()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_middle() {
        assert_eq!(Redacted("secret").to_string(), "s****t");
    }

    #[test]
    fn collapses_short() {
        assert_eq!(Redacted("a").to_string(), "**");
        assert_eq!(Redacted("ab").to_string(), "**");
        assert_eq!(Redacted("").to_string(), "<empty>");
    }

    #[test]
    fn path_keeps_filename() {
        assert_eq!(redact_path("/home/alice/movies/v.mkv"), "<redacted>/v.mkv");
    }

    #[test]
    fn path_handles_bare() {
        assert_eq!(redact_path("/"), "<redacted-path>");
    }
}
