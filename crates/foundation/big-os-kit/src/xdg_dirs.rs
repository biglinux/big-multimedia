// SPDX-License-Identifier: MIT

//! XDG directory resolution for Linux desktop apps.
//!
//! This module intentionally avoids the cross-platform `dirs` crate. BigLinux
//! packages target Linux/XDG, so resolving the relevant environment variables
//! directly keeps the dependency graph smaller and avoids unrelated platform
//! transitive crates.

use std::path::PathBuf;

/// Return `$HOME` when it is set to an absolute path.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    absolute_env_path("HOME")
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

/// Return `$XDG_STATE_HOME` or `$HOME/.local/state`.
#[must_use]
pub fn state_dir() -> Option<PathBuf> {
    xdg_base_dir("XDG_STATE_HOME", ".local/state")
}

fn xdg_base_dir(env_key: &str, home_relative: &str) -> Option<PathBuf> {
    absolute_env_path(env_key).or_else(|| home_dir().map(|home| home.join(home_relative)))
}

fn absolute_env_path(key: &str) -> Option<PathBuf> {
    let value = std::env::var_os(key)?;
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}
