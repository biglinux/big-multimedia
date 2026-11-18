// SPDX-License-Identifier: MIT

//! XDG directory resolution for Linux desktop apps.
//!
//! This module avoids the cross-platform `dirs` crate. BigLinux
//! packages target Linux/XDG, so resolving the relevant environment variables
//! directly keeps the dependency graph smaller and avoids unrelated platform
//! transitive crates.

use std::ffi::OsString;
use std::path::PathBuf;

/// Return `$HOME` when it is set to an absolute path.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    absolute_path(std::env::var_os("HOME"))
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

    #[test]
    fn unusable_home_resolves_nothing() {
        for home in [None, Some("home/u"), Some("")] {
            assert_eq!(resolve(Some("cache"), home), None);
        }
    }
}
