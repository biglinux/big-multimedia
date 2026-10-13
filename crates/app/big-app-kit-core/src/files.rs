// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Filename sanitizing and display-name helpers.

use std::path::Path;

/// Turn arbitrary display text or an URL into a bounded filename stem.
#[must_use]
pub fn safe_filename_stem(input: &str, max_len: usize, fallback: &str) -> String {
    let mut out = String::with_capacity(input.len().min(max_len));
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if "-_.".contains(ch) {
            out.push(ch);
        } else {
            out.push('_');
        }
        if out.len() >= max_len {
            out.truncate(max_len);
            break;
        }
    }
    if out.is_empty() {
        out.push_str(fallback);
        out.truncate(max_len);
    }
    out
}

/// Sanitize a user-facing filename stem while preserving Unicode letters/numbers.
#[must_use]
pub fn sanitize_filename_stem_preserve_unicode(input: &str, fallback: &str) -> String {
    let safe = input
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character == ' ' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .replace(' ', "_");
    if safe.is_empty() {
        fallback.to_string()
    } else {
        safe
    }
}

/// Return the file stem of `path` (basename without final extension);
/// falls back to the raw input when no stem is recoverable.
#[must_use]
pub fn file_stem_display_name(path: &str) -> String {
    Path::new(path).file_stem().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().to_string(),
    )
}

/// Return the basename of `path` for use as a folder label; falls back
/// to the raw input for roots like `"/"`.
#[must_use]
pub fn folder_display_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_stem_is_lowercase_and_bounded() {
        assert_eq!(safe_filename_stem("My IPTV", 80, "unnamed"), "my_iptv");
        assert_eq!(
            safe_filename_stem("http://a.example/x.m3u", 80, "unnamed"),
            "http___a.example_x.m3u"
        );
        assert_eq!(safe_filename_stem("", 80, "unnamed"), "unnamed");
        assert_eq!(
            safe_filename_stem(&"a".repeat(200), 80, "unnamed").len(),
            80
        );
    }

    #[test]
    fn unicode_stem_keeps_letters() {
        assert_eq!(
            sanitize_filename_stem_preserve_unicode("Rádio Brasil", "radio"),
            "Rádio_Brasil"
        );
        assert_eq!(
            sanitize_filename_stem_preserve_unicode("Rock & Roll!", "radio"),
            "Rock___Roll_"
        );
        assert_eq!(
            sanitize_filename_stem_preserve_unicode("  Radio  ", "radio"),
            "Radio"
        );
        assert_eq!(
            sanitize_filename_stem_preserve_unicode("", "radio"),
            "radio"
        );
    }

    #[test]
    fn stem_name_strips_path_and_extension() {
        assert_eq!(file_stem_display_name("/home/user/video.mp4"), "video");
        assert_eq!(file_stem_display_name("video.mp4"), "video");
        assert_eq!(file_stem_display_name("/home/user/folder"), "folder");
    }

    #[test]
    fn folder_name_falls_back_to_path() {
        assert_eq!(folder_display_name("/home/user/Videos"), "Videos");
        assert_eq!(folder_display_name("/"), "/");
        assert_eq!(folder_display_name("myfolder"), "myfolder");
    }
}
