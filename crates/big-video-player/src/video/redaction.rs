//! PII redaction for log / tracing output.
//!
//! Use `redact_path()` for filesystem paths — keeps the leaf filename (and its
//! ext) but hides the parent directory tree.

use std::path::Path;

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
    fn path_keeps_filename() {
        assert_eq!(redact_path("/home/alice/movies/v.mkv"), "<redacted>/v.mkv");
    }

    #[test]
    fn path_handles_bare() {
        assert_eq!(redact_path("/"), "<redacted-path>");
    }
}
