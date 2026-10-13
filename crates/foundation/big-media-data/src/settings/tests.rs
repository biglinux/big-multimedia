use super::*;

#[test]
fn color_source_default_is_gtk_default() {
    let defaults = Settings::defaults();
    assert_eq!(
        defaults.get("audio-ui-color-source").map(String::as_str),
        Some("gtk-default"),
    );
}

#[test]
fn color_source_round_trip_all_values() {
    let settings = Settings::memory_only();
    for value in ["gtk-default", "gtk-light", "gtk-dark", "gradient", "custom"] {
        settings.put_string(keys::AUDIO_UI_COLOR_SOURCE, value);
        assert_eq!(
            settings.get_string(keys::AUDIO_UI_COLOR_SOURCE),
            value,
            "round-trip failed for color source '{value}'",
        );
    }
}

#[test]
fn video_and_audio_color_source_are_independent() {
    let settings = Settings::memory_only();
    settings.put_string(keys::UI_COLOR_SOURCE, "gtk-dark");
    settings.put_string(keys::AUDIO_UI_COLOR_SOURCE, "gtk-light");
    assert_eq!(settings.get_string(keys::UI_COLOR_SOURCE), "gtk-dark");
    assert_eq!(
        settings.get_string(keys::AUDIO_UI_COLOR_SOURCE),
        "gtk-light"
    );
}

#[test]
fn invalid_values_leave_state_unchanged() {
    let settings = Settings::memory_only();
    assert!(!settings.set_double("volume", f64::NAN));
    assert!(!settings.set_string("title", "one\ntwo"));
    assert!(!settings.set_string("volume", "not-a-number"));
    assert!(settings.set_double("volume", 0.0));
    assert!(settings.try_flush().is_err()); // never use /tmp as HOME fallback
}

#[test]
fn shared_handles_keep_external_edits() {
    let path = std::env::temp_dir().join(format!("settings-shared-{}.conf", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let a = Settings::from_path(path.clone());
    let b = Settings::from_path(path.clone());
    a.put_double(keys::VOLUME, 0.0);
    assert_eq!(b.get_double(keys::VOLUME), 0.0);
    b.try_flush().unwrap();
    std::fs::write(&path, b"volume=21\n").unwrap();
    a.put_double(keys::VOLUME, 30.0);
    assert!(a.try_flush().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"volume=21\n");
    assert_eq!(a.get_double(keys::VOLUME), 30.0);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(
        path.with_file_name(format!("settings-shared-{}.conf.lock", std::process::id())),
    );
}

#[test]
fn memory_only_never_confirms_saves() {
    let settings = Settings::memory_only();
    assert!(settings.try_flush().is_err());
    assert_eq!(settings.save_status().phase, SavePhase::Failed);
}

#[test]
fn defaults_pass_the_schema() {
    for (key, value) in Settings::defaults() {
        assert!(schema::valid(&key, &value), "{key}={value}");
    }
}

#[test]
fn settings_round_trip() {
    let settings = Settings::memory_only();
    settings.put_bool(keys::MUTE, true);
    assert!(settings.get_bool(keys::MUTE));
    settings.put_int(keys::APPEARANCE_WINDOW_WIDTH, 1234);
    assert_eq!(settings.get_int(keys::APPEARANCE_WINDOW_WIDTH), 1234);
    settings.put_double(keys::VOLUME, 0.75);
    assert!((settings.get_double(keys::VOLUME) - 0.75).abs() < f64::EPSILON);
    settings.put_string(keys::HWDEC, "vaapi");
    assert_eq!(settings.get_string(keys::HWDEC), "vaapi");
}
