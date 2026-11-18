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
fn video_colors_are_the_video_players_default() {
    let defaults = Settings::defaults();
    assert_eq!(
        defaults.get("ui-color-source").map(String::as_str),
        Some("gradient"),
    );
}

#[test]
fn audio_migration_leaves_video_colors_behind() {
    let settings = Settings::memory_only();
    settings.migrate_audio_appearance();
    assert_eq!(
        settings.get_string(keys::AUDIO_UI_COLOR_SOURCE),
        "gtk-default"
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

#[test]
fn volume_max_stays_in_preference_range() {
    let settings = Settings::memory_only();
    assert_eq!(settings.volume_max_percent(), 150);
    for (stored, expected) in [
        (0, 100),
        (90, 100),
        (100, 100),
        (180, 180),
        (200, 200),
        (250, 200),
    ] {
        settings.put_int(keys::VOLUME_MAX, stored);
        assert_eq!(settings.volume_max_percent(), expected, "stored {stored}");
    }
}

#[test]
fn serialized_settings_read_back_unchanged() {
    const KEYS: [&str; 8] = [
        "mute",
        "tray-icon",
        "volume",
        "subtitle-font-size",
        "render-mode",
        "sub-font",
        "hwdec",
        "x-unknown_key",
    ];
    const PIECES: [&str; 14] = [
        " ", "a", "Z", "0", "1", "=", "#", "é", "€", ".", "-", "true", "auto", "sw",
    ];
    // A fixed linear congruential generator: the same maps on every run.
    let mut state: u64 = 0x5eed;
    let mut below = |n: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as usize % n
    };
    for _ in 0..2000 {
        let mut map = HashMap::new();
        for _ in 0..below(6) {
            let key = KEYS[below(KEYS.len())];
            let value: String = (0..below(5)).map(|_| PIECES[below(PIECES.len())]).collect();
            if schema::valid(key, &value) {
                map.insert(key.to_owned(), value);
            }
        }
        let bytes = store::serialize(&map).unwrap();
        assert_eq!(store::parse(&bytes).unwrap(), map, "{map:?}");
    }
}

#[test]
fn hand_edited_lines_keep_their_values() {
    let parsed = store::parse(b"# comment\n\n  volume =0.5\nsub-font=Noto Sans \n").unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed["volume"], "0.5");
    assert_eq!(parsed["sub-font"], "Noto Sans ");
}

#[test]
fn schema_types_tray_icon_and_render_mode() {
    for (key, value, valid) in [
        ("tray-icon", "true", true),
        ("tray-icon", "false", true),
        ("tray-icon", "yes", false),
        ("render-mode", "auto", true),
        ("render-mode", "gl", true),
        ("render-mode", "sw", true),
        ("render-mode", "vulkan", false),
    ] {
        assert_eq!(schema::valid(key, value), valid, "{key}={value}");
    }
}
