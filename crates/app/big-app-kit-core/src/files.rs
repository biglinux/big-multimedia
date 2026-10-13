// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! File picker specs and display-name helpers.

use std::path::Path;

/// Behaviour selected for a [`BigFilePickerSpec`]; drives which portal
/// method the adapter calls and which response fields it expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigPickerMode {
    /// Pick one or more existing files to open.
    OpenFile,
    /// Pick an existing folder to open.
    OpenFolder,
    /// Pick a filesystem location for a new file (save dialog).
    SaveFile,
    /// Pick an existing folder as the destination for output files.
    DestinationFolder,
}

/// Typed file picker spec. Drives portal- or libadwaita-backed file dialogs.
///
/// # Capabilities
///
/// `file-picker`, `file-picker+portal`
///
/// # Archetypes
///
/// `media-converter`, `editor`, `file-manager`, `screenshot`, `clipboard`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigFilePickerSpec {
    /// Behaviour selected for this picker.
    pub mode: BigPickerMode,
    /// Localized dialog title shown by the portal.
    pub title: String,
    /// `true` to allow multi-selection (only honoured in
    /// `OpenFile`/`OpenFolder` modes).
    pub multiple: bool,
    /// Glob patterns (e.g. `"*.mp4"`) the dialog should filter by.
    pub patterns: Vec<String>,
    /// `true` to require the desktop portal backend; `false` lets the
    /// adapter fall back to a libadwaita-only dialog in test environments.
    pub portal_required: bool,
}

impl BigFilePickerSpec {
    /// Build a picker spec. Portal use is required by default; patterns
    /// and multi-select start empty.
    #[must_use]
    pub fn new(mode: BigPickerMode, title: impl Into<String>) -> Self {
        Self {
            mode,
            title: title.into(),
            multiple: false,
            patterns: Vec::new(),
            portal_required: true,
        }
    }

    /// Shortcut for an "Open file" toolbar button picker.
    #[must_use]
    pub fn open_file_button(title: impl Into<String>) -> Self {
        Self::new(BigPickerMode::OpenFile, title)
    }

    /// Shortcut for an "Open folder" picker.
    #[must_use]
    pub fn open_folder_button(title: impl Into<String>) -> Self {
        Self::new(BigPickerMode::OpenFolder, title)
    }

    /// Shortcut for a "Save as" picker.
    #[must_use]
    pub fn save_file_button(title: impl Into<String>) -> Self {
        Self::new(BigPickerMode::SaveFile, title)
    }

    /// Enable or disable multi-selection. Only meaningful in
    /// `OpenFile`/`OpenFolder` modes.
    #[must_use]
    pub fn multiple(mut self, enabled: bool) -> Self {
        self.multiple = enabled;
        self
    }
}

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
