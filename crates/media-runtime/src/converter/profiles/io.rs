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

fn user_profiles_dir() -> Option<PathBuf> {
    big_os_kit::xdg_dirs::config_dir().map(|p|p.join(crate::config::runtime::USER_PROFILES_RELATIVE))
}

pub fn load_profile(path: &Path) -> Result<Profile> {
    let bytes=big_os_kit::storage::read_regular_limited(path,1024*1024)?
        .ok_or_else(||io::Error::new(io::ErrorKind::NotFound,"profile not found"))?;
    let content=std::str::from_utf8(&bytes).map_err(|_|Error::invalid_argument("profile","Profile is not UTF-8"))?;
    toml::from_str(content).map_err(|_|Error::invalid_argument("profile","Invalid profile document"))
}

// Intentionally returns `io::Error` rather than the crate `Error`: callers
// match on `io::ErrorKind::PermissionDenied` / `NotFound` etc. directly
// (XDG profile dir handling needs the underlying kind, not a string
// wrap). Keep this narrower type until the matching call-sites migrate.
/// Worker-only, create-new publication. Use save_user_profile_async from GTK.
pub fn save_user_profile(
    filename: &str,
    profile: &Profile,
) -> std::result::Result<PathBuf, io::Error> {
    let name=Path::new(filename);
    if name.components().count()!=1 || name.file_name().and_then(|n|n.to_str())!=Some(filename) || name.extension().and_then(|n|n.to_str())!=Some("toml") {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"Choose a plain .toml profile filename"));
    }
    let dir=user_profiles_dir().ok_or_else(||io::Error::new(io::ErrorKind::NotFound,"User configuration directory is unavailable"))?;
    fs::create_dir_all(&dir)?;
    let path=dir.join(filename);
    let content = toml::to_string_pretty(profile).map_err(|e| io::Error::other(e.to_string()))?;
    if content.len()>1024*1024 {return Err(io::Error::new(io::ErrorKind::InvalidInput,"Profile exceeds 1 MiB"));}
    let stage=big_os_kit::storage::StagedFile::new(&path)?;
    fs::write(stage.path(),content)?;
    stage.publish(&path)?.into_io_result()?;
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

    if let Some(dir)=user_profiles_dir() {load_profiles_from_dir(&dir,&mut profiles);}

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

/// GUI-safe profile export with an acknowledged result. Existing files survive.
pub fn save_user_profile_async(filename:String,profile:Profile,done:impl FnOnce(io::Result<PathBuf>)+'static) {
    crate::settings::saving::acknowledged_task(false, move ||save_user_profile(&filename,&profile),done);
}
