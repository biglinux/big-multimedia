//! Profile TOML loader: layered system + dev-fallback + user with user
//! winning on filename collision. Dev-fallback walks exe-relative paths so
//! `cargo run` finds bundled profiles without an install step.

use log::{info, warn};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fs, io};

use super::Profile;
use crate::error::{Error, Result};

const SYSTEM_PROFILES_DIR: &str = crate::config::runtime::PROFILES_DIR;

fn user_profiles_dir() -> PathBuf {
    big_os_kit::xdg_dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(crate::config::runtime::USER_PROFILES_RELATIVE)
}

pub fn load_profile(path: &Path) -> Result<Profile> {
    let content = fs::read_to_string(path)?;
    toml::from_str(&content)
        .map_err(|e| Error::invalid_argument("profile", format!("parse {}: {e}", path.display())))
}

// Intentionally returns `io::Error` rather than the crate `Error`: callers
// match on `io::ErrorKind::PermissionDenied` / `NotFound` etc. directly
// (XDG profile dir handling needs the underlying kind, not a string
// wrap). Keep this narrower type until the matching call-sites migrate.
pub fn save_user_profile(
    filename: &str,
    profile: &Profile,
) -> std::result::Result<PathBuf, io::Error> {
    let dir = user_profiles_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(filename);
    let content = toml::to_string_pretty(profile).map_err(|e| io::Error::other(e.to_string()))?;
    fs::write(&path, content)?;
    Ok(path)
}

// User filenames overwrite system entries; BTreeMap keeps deterministic order.
#[must_use]
pub fn load_all_profiles() -> BTreeMap<String, Profile> {
    let mut profiles = BTreeMap::new();

    load_profiles_from_dir(Path::new(SYSTEM_PROFILES_DIR), &mut profiles);

    if profiles.is_empty() {
        load_dev_profiles(&mut profiles);
    }

    load_profiles_from_dir(&user_profiles_dir(), &mut profiles);

    info!("Loaded {} profiles", profiles.len());
    profiles
}

fn load_dev_profiles(profiles: &mut BTreeMap<String, Profile>) {
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        let dev_paths = [
            exe_dir.join("../../data/profiles"),
            exe_dir.join("../data/profiles"),
            exe_dir.join("data/profiles"),
        ];
        for dev in &dev_paths {
            if dev.is_dir() {
                info!("Dev profiles dir: {}", dev.display());
                load_profiles_from_dir(dev, profiles);
                if !profiles.is_empty() {
                    return;
                }
            }
        }
    }

    let cwd_profiles = Path::new("data/profiles");
    if cwd_profiles.is_dir() {
        load_profiles_from_dir(cwd_profiles, profiles);
    }
}

fn load_profiles_from_dir(dir: &Path, profiles: &mut BTreeMap<String, Profile>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            let filename = entry.file_name().to_string_lossy().to_string();
            match load_profile(&path) {
                Ok(profile) => {
                    profiles.insert(filename, profile);
                }
                Err(e) => {
                    warn!("Skip profile {}: {e}", path.display());
                }
            }
        }
    }
}
