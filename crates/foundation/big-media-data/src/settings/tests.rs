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
fn catalog_names_each_key_once() {
    let mut names: Vec<&str> = keys::CATALOG.iter().map(|entry| entry.name).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(names.len(), count);
}

/// The key tables of docs/settings.md state each key's type and default.
#[test]
fn settings_doc_lists_the_catalog() {
    let doc = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../docs/settings.md"
    ));
    let tables = &doc[doc.find("## Keys").unwrap()..doc.find("## Environment variables").unwrap()];
    let mut documented = HashMap::new();
    for row in tables.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = row.split(" | ").collect();
        let name = cells[0].trim_start_matches("| ").trim_matches('`');
        let listed = (cells[1].to_owned(), cells[2].to_owned());
        assert!(
            documented.insert(name, listed).is_none(),
            "{name} listed twice"
        );
    }
    for entry in keys::CATALOG {
        let value_type = if entry.choices.is_empty() {
            match entry.value_type {
                keys::ValueType::Bool => "bool".to_owned(),
                keys::ValueType::Int => "integer".to_owned(),
                keys::ValueType::Double => "number".to_owned(),
                keys::ValueType::Text => "string".to_owned(),
            }
        } else {
            let choices: Vec<String> = entry.choices.iter().map(|c| format!("`{c}`")).collect();
            choices.join(", ")
        };
        let default = if entry.default.is_empty() {
            "empty".to_owned()
        } else {
            format!("`{}`", entry.default)
        };
        assert_eq!(
            documented.remove(entry.name),
            Some((value_type, default)),
            "{}",
            entry.name
        );
    }
    assert!(documented.is_empty(), "not in the catalog: {documented:?}");
}

#[test]
fn settings_round_trip() {
    let settings = Settings::memory_only();
    settings.put_bool(keys::DEINTERLACE, true);
    assert!(settings.get_bool(keys::DEINTERLACE));
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
fn saved_volume_keeps_zero_within_maximum() {
    let settings = Settings::memory_only();
    assert_eq!(settings.audio_volume_percent(), 100.0);
    assert_eq!(settings.video_volume_percent(), 100.0);
    for (saved, expected) in [(0.0, 0.0), (37.0, 37.0), (150.0, 150.0), (180.0, 150.0)] {
        settings.put_audio_volume_percent(saved);
        settings.put_video_volume_percent(saved);
        assert_eq!(settings.audio_volume_percent(), expected, "audio {saved}");
        assert_eq!(settings.video_volume_percent(), expected, "video {saved}");
    }
    settings.put_int(keys::VOLUME_MAX, 200);
    assert_eq!(settings.audio_volume_percent(), 180.0);
    assert_eq!(settings.get_int(keys::AUDIO_VOLUME), 180);
    assert_eq!(settings.get_double(keys::VOLUME), 1.8);
    settings.put_int(keys::AUDIO_VOLUME, -5);
    assert_eq!(settings.audio_volume_percent(), 0.0);
}

#[test]
fn serialized_settings_read_back_unchanged() {
    const KEYS: [&str; 8] = [
        "deinterlace",
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
    for (line, key, value) in [
        ("  volume =0.5", "volume", "0.5"),
        ("volume= 80", "volume", "80"),
        ("deinterlace=true ", "deinterlace", "true"),
        ("subtitle-font-size= 36 ", "subtitle-font-size", "36"),
        ("render-mode= sw", "render-mode", "sw"),
        ("sub-font=Noto Sans ", "sub-font", "Noto Sans "),
        ("x-unknown_key= kept ", "x-unknown_key", " kept "),
    ] {
        let parsed = store::parse(format!("# comment\n\n{line}\n").as_bytes()).unwrap();
        assert_eq!(parsed.len(), 1, "{line}");
        assert_eq!(parsed[key], value, "{line}");
    }
}

#[test]
fn schema_types_tray_icon_and_render_mode() {
    for (key, value, valid) in [
        ("tray-icon", "true", true),
        ("tray-icon", "false", true),
        ("tray-icon", "yes", false),
        ("playlist-reorder-hint-seen", "yes", false),
        ("render-mode", "auto", true),
        ("render-mode", "gl", true),
        ("render-mode", "sw", true),
        ("render-mode", "vulkan", false),
    ] {
        assert_eq!(schema::valid(key, value), valid, "{key}={value}");
    }
}

#[test]
fn removed_keys_load_as_unknown_text() {
    let file = "mute=maybe\nscreenshot-jpeg-quality=high\ntarget-contrast=auto\n\
                hdr-contrast-recovery=none\ntarget-colorspace-hint=1\nicc-profile-auto=yes\n\
                secondary-subtitles=on\nscreenshot-format=webp\nscreenshot-template=%f\n";
    let parsed = store::parse(file.as_bytes()).unwrap();
    assert_eq!(parsed.len(), 9);
    assert_eq!(parsed["mute"], "maybe");
    assert_eq!(parsed["target-contrast"], "auto");
    for key in parsed.keys() {
        assert!(!Settings::defaults().contains_key(key), "{key}");
    }
}

#[test]
fn restore_defaults_keeps_user_data() {
    let settings = Settings::memory_only();
    let reset = [
        ("render-mode", "sw", "auto"),
        ("tray-icon", "true", "false"),
        ("audio-album-art-bg", "false", "true"),
        ("shader-fsr-sharpness", "0.5", "0"),
        ("shader-cas-sharpening", "0.5", "0"),
        ("hwdec", "vaapi", "auto-safe"),
        ("ui-show-volume", "false", "true"),
        ("volume-max", "200", "150"),
    ];
    let kept = [
        ("video-library-folders", r#"["/videos"]"#),
        ("custom-keybindings", "mute=m"),
        ("volume", "0.4"),
        ("files-last-dir", "/music"),
        ("playlist-last-folder", "/videos"),
        ("playlist-shuffle", "true"),
        ("eq-bands", "1;2;3;4;5;6;7;8;9;10"),
        ("noise-strength", "0.5"),
        ("appearance-window-width", "900"),
    ];
    for (key, value, _) in reset {
        assert!(settings.set_string(key, value), "{key}");
    }
    for (key, value) in kept {
        assert!(settings.set_string(key, value), "{key}");
    }
    settings.reset_to_defaults();
    for (key, _, default) in reset {
        assert_eq!(settings.string(key), default, "{key}");
    }
    for (key, value) in kept {
        assert_eq!(settings.string(key), value, "{key}");
    }
}

#[test]
fn requested_flush_reaches_disk() {
    let name = format!("settings-request-{}.conf", std::process::id());
    let path = std::env::temp_dir().join(&name);
    let _ = std::fs::remove_file(&path);
    let settings = Settings::from_path(path.clone());
    settings.put_double(keys::VOLUME, 42.0);
    assert!(Settings::any_unsaved());
    settings.request_flush().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while settings.save_status().phase != SavePhase::Saved {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            settings.save_status()
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let saved = store::parse(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.get("volume"), Some(&settings.string("volume")));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_file_name(format!("{name}.lock")));
}

/// Each refused row breaks one rule; the accepted rows sit at the caps.
#[test]
fn schema_bounds_keys_and_values() {
    let long_key = "k".repeat(128);
    let long_value = "v".repeat(65_536);
    for (key, value) in [(long_key.as_str(), "x"), ("x-text", long_value.as_str())] {
        assert_eq!(schema::normalized(key, value), Some(value), "{key}");
    }
    let longer_key = "k".repeat(129);
    let longer_value = "v".repeat(65_537);
    for (key, value) in [
        ("", "x"),
        (longer_key.as_str(), "x"),
        ("x-text", longer_value.as_str()),
        ("bad key", "x"),
        ("x-text", "a\u{7}b"),
    ] {
        assert_eq!(schema::normalized(key, value), None, "{key:?}={value:?}");
    }
    let invalid = HashMap::from([("volume".to_owned(), "loud".to_owned())]);
    assert!(store::serialize(&invalid).is_err());
}

#[test]
fn noise_voice_only_follows_switch() {
    let settings = Settings::memory_only();
    assert!(!settings.get_bool(keys::NOISE_VOICE_ONLY));
    assert_eq!(settings.noise_voice_only(), None);
    settings.put_bool(keys::NOISE_VOICE_ONLY, true);
    assert_eq!(settings.noise_voice_only(), Some(3));
    assert!(!settings.put_int(keys::NOISE_VOICE_SENSITIVITY, 9));
    settings.put_int(keys::NOISE_VOICE_SENSITIVITY, 2);
    assert_eq!(settings.noise_voice_only(), Some(2));
}

/// The file cap is 1 MiB; one text value is at most 64 KiB.
#[test]
fn serialize_refuses_oversized_file() {
    let map = |count: usize| {
        (0..count)
            .map(|i| (format!("x-text-{i:02}"), "v".repeat(65_536)))
            .collect::<HashMap<_, _>>()
    };
    assert!(store::serialize(&map(15)).is_ok());
    assert!(store::serialize(&map(17)).is_err());
}
