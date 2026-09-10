use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use log::error;

use super::{Inner, defaults, keys};
use crate::error::Result;

fn save_to_file(inner: &Inner) {
    if let Some(dir) = inner.path.parent()
        && let Err(e) = fs::create_dir_all(dir)
    {
        error!("Failed to create config dir: {e}");
        return;
    }

    let mut entries: Vec<_> = inner.settings_values.iter().collect();
    entries.sort_by_key(|(key, _)| *key);

    let mut content = String::new();
    for (key, value) in entries {
        content.push_str(key);
        content.push('=');
        content.push_str(value);
        content.push('\n');
    }

    // Atomic write: a crash mid-write must not truncate or corrupt the user's
    // settings. Write to a sibling temp file, flush it to disk, then rename
    // over the target (rename is atomic on the same filesystem).
    if let Err(e) = write_atomic(&inner.path, content.as_bytes()) {
        error!("Failed to save settings: {e}");
    }
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let temporary_settings_path = path.with_extension("conf.tmp");
    {
        let mut file = fs::File::create(&temporary_settings_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    match fs::rename(&temporary_settings_path, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&temporary_settings_path);
            Err(e)
        }
    }
}

#[derive(Clone)]
pub struct Settings {
    inner: Arc<RwLock<Inner>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self::new(crate::config::runtime::CONFIG_DIR)
    }
}

impl Settings {
    #[must_use]
    pub fn for_audio() -> Self {
        Self::new(crate::config::audio::CONFIG_DIR)
    }

    #[must_use]
    pub fn for_video() -> Self {
        Self::new(crate::config::video::CONFIG_DIR)
    }

    /// Stores at `~/.config/<app_name>/settings.conf`.
    #[must_use]
    pub fn new(app_name: &str) -> Self {
        let config_dir = big_os_kit::xdg_dirs::config_dir()
            .unwrap_or_else(|| {
                big_os_kit::xdg_dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(".config")
            })
            .join(app_name);
        let path = config_dir.join("settings.conf");

        let mut settings_values = defaults::defaults();
        if let Ok(content) = fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    settings_values.insert(key.trim().to_string(), value.to_string());
                }
            }
        }

        Self {
            inner: Arc::new(RwLock::new(Inner {
                settings_values,
                path,
                dirty: false,
            })),
        }
    }

    pub(crate) fn defaults() -> std::collections::HashMap<String, String> {
        defaults::defaults()
    }

    #[inline]
    #[allow(unused_variables, clippy::unused_self)]
    fn debug_check_key(&self, key: &str) {
        #[cfg(debug_assertions)]
        {
            if key.starts_with("dlg-") {
                return;
            }
            let inner = self
                .inner
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !inner.settings_values.contains_key(key) {
                log::warn!("Settings: unknown key {key:?} — consider adding to defaults()");
            }
        }
    }

    #[must_use]
    pub fn string(&self, key: &str) -> String {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.settings_values.get(key).cloned().unwrap_or_default()
    }

    #[must_use]
    pub fn boolean(&self, key: &str) -> bool {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .get(key)
            .is_some_and(|value| value == "true")
    }

    #[must_use]
    pub fn boolean_or(&self, key: &str, default: bool) -> bool {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.settings_values.get(key) {
            Some(value) => value == "true",
            None => default,
        }
    }

    #[must_use]
    pub fn int(&self, key: &str) -> i32 {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .get(key)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    }

    #[must_use]
    pub fn double(&self, key: &str) -> f64 {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .get(key)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0)
    }

    pub fn set_string(&self, key: &str, value: &str) -> bool {
        self.debug_check_key(key);
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .insert(key.to_string(), value.to_string());
        inner.dirty = true;
        true
    }

    pub fn set_boolean(&self, key: &str, value: bool) -> bool {
        self.debug_check_key(key);
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .insert(key.to_string(), value.to_string());
        inner.dirty = true;
        true
    }

    pub fn set_int(&self, key: &str, value: i32) -> bool {
        self.debug_check_key(key);
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .insert(key.to_string(), value.to_string());
        inner.dirty = true;
        true
    }

    pub fn set_double(&self, key: &str, value: f64) -> bool {
        self.debug_check_key(key);
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .insert(key.to_string(), value.to_string());
        inner.dirty = true;
        true
    }

    #[must_use]
    pub fn get_bool(&self, key: keys::BoolKey) -> bool {
        self.boolean(key.0)
    }

    #[must_use]
    pub fn get_bool_or(&self, key: keys::BoolKey, default: bool) -> bool {
        self.boolean_or(key.0, default)
    }

    #[must_use]
    pub fn get_int(&self, key: keys::IntKey) -> i32 {
        self.int(key.0)
    }

    #[must_use]
    pub fn get_double(&self, key: keys::DoubleKey) -> f64 {
        self.double(key.0)
    }

    #[must_use]
    pub fn get_string(&self, key: keys::StringKey) -> String {
        self.string(key.0)
    }

    pub fn put_bool(&self, key: keys::BoolKey, value: bool) -> bool {
        self.set_boolean(key.0, value)
    }

    pub fn put_int(&self, key: keys::IntKey, value: i32) -> bool {
        self.set_int(key.0, value)
    }

    pub fn put_double(&self, key: keys::DoubleKey, value: f64) -> bool {
        self.set_double(key.0, value)
    }

    pub fn put_string(&self, key: keys::StringKey, value: &str) -> bool {
        self.set_string(key.0, value)
    }

    pub fn flush(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.dirty {
            save_to_file(&inner);
            inner.dirty = false;
        }
    }

    /// One-time copy of `ui-*` colors to `audio-ui-*` so existing users keep
    /// their video-player palette in the audio binary without re-applying it.
    pub fn migrate_audio_appearance(&self) {
        let needs_migrate = {
            let inner = self
                .inner
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let defaults = Self::defaults();
            let pairs = [
                ("audio-ui-color-source", "ui-color-source"),
                ("audio-ui-bg-color", "ui-bg-color"),
                ("audio-ui-font-color", "ui-font-color"),
                ("audio-ui-icon-color", "ui-icon-color"),
                ("audio-ui-dialog-color", "ui-dialog-color"),
            ];
            let mut changed = false;
            for (audio_key, video_key) in &pairs {
                let audio_value = inner
                    .settings_values
                    .get(*audio_key)
                    .cloned()
                    .unwrap_or_default();
                let default_value = defaults.get(*audio_key).cloned().unwrap_or_default();
                let video_value = inner
                    .settings_values
                    .get(*video_key)
                    .cloned()
                    .unwrap_or_default();
                if audio_value == default_value && video_value != default_value {
                    changed = true;
                    break;
                }
            }
            if changed { Some(pairs) } else { None }
        };

        if let Some(pairs) = needs_migrate {
            let mut inner = self
                .inner
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (audio_key, video_key) in &pairs {
                if let Some(value) = inner.settings_values.get(*video_key).cloned() {
                    inner
                        .settings_values
                        .insert((*audio_key).to_string(), value);
                }
            }
            inner.dirty = true;
            save_to_file(&inner);
            inner.dirty = false;
        }
    }

    pub fn reset_to_defaults(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let defaults = Self::defaults();
        for (key, value) in &defaults {
            match key.as_str() {
                "window-width" | "window-height" | "window-maximized" => continue,
                _ => {}
            }
            inner.settings_values.insert(key.clone(), value.clone());
        }
        save_to_file(&inner);
        inner.dirty = false;
    }

    pub fn export_to(&self, path: &str) -> Result<()> {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut entries: Vec<_> = inner.settings_values.iter().collect();
        entries.sort_by_key(|(key, _)| *key);
        let mut content = String::new();
        for (key, value) in entries {
            content.push_str(key);
            content.push('=');
            content.push_str(value);
            content.push('\n');
        }
        fs::write(path, &content)?;
        Ok(())
    }

    pub fn import_from(&self, path: &str) -> Result<()> {
        let content = fs::read_to_string(path)?;
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                inner
                    .settings_values
                    .insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        save_to_file(&inner);
        inner.dirty = false;
        Ok(())
    }
}
