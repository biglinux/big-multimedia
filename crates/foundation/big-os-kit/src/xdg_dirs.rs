// SPDX-License-Identifier: MIT

//! XDG directory resolution for Linux desktop apps.
//!
//! This module avoids the cross-platform `dirs` crate and GLib's lookups.
//! BigLinux packages target Linux/XDG, so resolving the relevant environment
//! variables and `user-dirs.dirs` directly keeps the dependency graph smaller.
//! Every function returns an absolute path or `None`, never a guess.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// `user-dirs.dirs` holds a dozen short lines; a larger file is not read.
const MAX_USER_DIRS_BYTES: usize = 64 * 1024;

/// Return the home folder: `$HOME` when set and not empty, otherwise the
/// user's passwd entry, as `glib::home_dir` does. A relative `$HOME` gives
/// `None`.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    #[expect(
        clippy::disallowed_methods,
        reason = "the owner of the home folder lookup"
    )]
    std::env::home_dir().filter(|home| home.is_absolute())
}

/// A folder that xdg-user-dirs names in `user-dirs.dirs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserDir {
    /// `XDG_MUSIC_DIR`.
    Music,
    /// `XDG_VIDEOS_DIR`.
    Videos,
    /// `XDG_PICTURES_DIR`.
    Pictures,
    /// `XDG_DOWNLOAD_DIR`.
    Downloads,
}

impl UserDir {
    fn key(self) -> &'static str {
        match self {
            Self::Music => "XDG_MUSIC_DIR",
            Self::Videos => "XDG_VIDEOS_DIR",
            Self::Pictures => "XDG_PICTURES_DIR",
            Self::Downloads => "XDG_DOWNLOAD_DIR",
        }
    }
}

/// Return the folder `$XDG_CONFIG_HOME/user-dirs.dirs` names for `dir`, or
/// `None` when the file or its entry is missing. The file is read once per
/// process, as GLib does.
#[must_use]
pub fn user_dir(dir: UserDir) -> Option<PathBuf> {
    static USER_DIRS: OnceLock<Option<String>> = OnceLock::new();
    let text = USER_DIRS
        .get_or_init(|| read_user_dirs(&config_dir()?.join("user-dirs.dirs")))
        .as_deref()?;
    parse_user_dir(text, dir.key(), home_dir().as_deref())
}

fn read_user_dirs(path: &Path) -> Option<String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!("{} could not be read: {error}", path.display());
            return None;
        }
    };
    let mut text = String::new();
    let limit = u64::try_from(MAX_USER_DIRS_BYTES + 1).unwrap_or(u64::MAX);
    if let Err(error) = file.take(limit).read_to_string(&mut text) {
        log::warn!("{} could not be read: {error}", path.display());
        return None;
    }
    if text.len() > MAX_USER_DIRS_BYTES {
        log::warn!(
            "{} is larger than {MAX_USER_DIRS_BYTES} bytes",
            path.display()
        );
        return None;
    }
    Some(text)
}

/// The last valid `KEY="value"` line wins. A value is `$HOME`, `$HOME/...` or
/// an absolute path in double quotes, where a backslash escapes the next
/// character; anything else is skipped as xdg-user-dirs and GLib skip it.
fn parse_user_dir(text: &str, key: &str, home: Option<&Path>) -> Option<PathBuf> {
    let mut found = None;
    for line in text.lines() {
        let Some((name, value)) = line.trim_start().split_once('=') else {
            continue;
        };
        if name.trim_end() == key
            && let Some(path) = user_dir_value(value.trim(), home)
        {
            found = Some(path);
        }
    }
    found
}

fn user_dir_value(value: &str, home: Option<&Path>) -> Option<PathBuf> {
    let mut chars = value.strip_prefix('"')?.chars();
    let mut unquoted = String::new();
    loop {
        match chars.next()? {
            '"' => break,
            '\\' => unquoted.push(chars.next()?),
            other => unquoted.push(other),
        }
    }
    let path = if let Some(rest) = unquoted.strip_prefix("$HOME") {
        if !rest.is_empty() && !rest.starts_with('/') {
            return None;
        }
        home?.join(rest.trim_start_matches('/'))
    } else {
        PathBuf::from(unquoted)
    };
    path.is_absolute().then(|| path.components().collect())
}

/// Return `$XDG_CONFIG_HOME` or `$HOME/.config`.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    xdg_base_dir("XDG_CONFIG_HOME", ".config")
}

/// Return `$XDG_CACHE_HOME` or `$HOME/.cache`.
#[must_use]
pub fn cache_dir() -> Option<PathBuf> {
    xdg_base_dir("XDG_CACHE_HOME", ".cache")
}

/// Return `$XDG_DATA_HOME` or `$HOME/.local/share`.
#[must_use]
pub fn data_dir() -> Option<PathBuf> {
    xdg_base_dir("XDG_DATA_HOME", ".local/share")
}

/// Return `$XDG_STATE_HOME` or `$HOME/.local/state`.
#[must_use]
pub fn state_dir() -> Option<PathBuf> {
    xdg_base_dir("XDG_STATE_HOME", ".local/state")
}

fn xdg_base_dir(env_key: &str, home_relative: &str) -> Option<PathBuf> {
    resolve_base_dir(
        std::env::var_os(env_key),
        std::env::var_os("HOME"),
        home_relative,
    )
}

/// The XDG Base Directory rule: a relative or empty value counts as unset.
fn resolve_base_dir(
    xdg_value: Option<OsString>,
    home: Option<OsString>,
    home_relative: &str,
) -> Option<PathBuf> {
    absolute_path(xdg_value).or_else(|| absolute_path(home).map(|home| home.join(home_relative)))
}

fn absolute_path(value: Option<OsString>) -> Option<PathBuf> {
    let path = PathBuf::from(value?);
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(xdg_value: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        resolve_base_dir(
            xdg_value.map(OsString::from),
            home.map(OsString::from),
            ".cache",
        )
    }

    #[test]
    fn absolute_xdg_value_wins_over_home() {
        assert_eq!(
            resolve(Some("/srv/cache"), Some("/home/u")),
            Some(PathBuf::from("/srv/cache"))
        );
    }

    #[test]
    fn unusable_xdg_value_uses_home() {
        for value in [Some("relative/cache"), Some(""), None] {
            assert_eq!(
                resolve(value, Some("/home/u")),
                Some(PathBuf::from("/home/u/.cache"))
            );
        }
    }

    fn home() -> Option<&'static Path> {
        Some(Path::new("/home/u"))
    }

    #[test]
    fn user_dir_reads_home_and_absolute_values() {
        let text = "# written by xdg-user-dirs-update\n\
            XDG_MUSIC_DIR=\"$HOME/Músicas\"\n\
            XDG_VIDEOS_DIR=\"/srv/videos/\"\n\
            XDG_DOWNLOAD_DIR=\"$HOME\"\n\
            XDG_PICTURES_DIR=\"$HOME/My \\\"Shots\\\"\"\n";
        let read = |key| parse_user_dir(text, key, home());
        assert_eq!(
            read("XDG_MUSIC_DIR"),
            Some(PathBuf::from("/home/u/Músicas"))
        );
        assert_eq!(read("XDG_VIDEOS_DIR"), Some(PathBuf::from("/srv/videos")));
        assert_eq!(read("XDG_DOWNLOAD_DIR"), Some(PathBuf::from("/home/u")));
        assert_eq!(
            read("XDG_PICTURES_DIR"),
            Some(PathBuf::from("/home/u/My \"Shots\""))
        );
        assert_eq!(read("XDG_TEMPLATES_DIR"), None);
    }

    #[test]
    fn user_dir_skips_values_it_cannot_place() {
        for value in [
            "relative/Music",
            "\"relative/Music\"",
            "\"$HOMEMusic\"",
            "\"/srv/unterminated",
            "",
        ] {
            let text = format!("XDG_MUSIC_DIR=\"/srv/music\"\nXDG_MUSIC_DIR={value}\n");
            assert_eq!(
                parse_user_dir(&text, "XDG_MUSIC_DIR", home()),
                Some(PathBuf::from("/srv/music")),
                "{value}"
            );
        }
        assert_eq!(
            parse_user_dir("XDG_MUSIC_DIR=\"$HOME/Music\"", "XDG_MUSIC_DIR", None),
            None
        );
        assert_eq!(parse_user_dir("", "XDG_MUSIC_DIR", home()), None);
    }

    #[test]
    fn oversized_user_dirs_file_ignored() {
        let path = std::env::temp_dir().join(format!("user-dirs-cap-{}", std::process::id()));
        let line = "XDG_MUSIC_DIR=\"/srv/music\"\n";
        let at_cap = format!("{line}{}", "#".repeat(MAX_USER_DIRS_BYTES - line.len()));
        std::fs::write(&path, &at_cap).unwrap();
        assert_eq!(read_user_dirs(&path).as_deref(), Some(at_cap.as_str()));
        std::fs::write(&path, format!("{at_cap}#")).unwrap();
        assert_eq!(read_user_dirs(&path), None);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_user_dirs(&path), None);
    }

    #[test]
    fn unusable_home_resolves_nothing() {
        for home in [None, Some("home/u"), Some("")] {
            assert_eq!(resolve(Some("cache"), home), None);
        }
    }
}
