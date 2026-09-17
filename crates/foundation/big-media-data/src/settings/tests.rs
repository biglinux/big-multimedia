use super::*;

/// Compile-only check — wrong key type would fail to build.
#[test]
fn typed_keys_compile_time_safety() {
    let settings = Settings::memory_only();

    let _ = settings.get_bool(keys::MUTE);
    settings.put_bool(keys::MUTE, false);

    let _ = settings.get_int(keys::WINDOW_WIDTH);
    settings.put_int(keys::WINDOW_WIDTH, 800);

    let _ = settings.get_double(keys::VOLUME);
    settings.put_double(keys::VOLUME, 0.5);

    let _ = settings.get_string(keys::HWDEC);
    settings.put_string(keys::HWDEC, "auto");
}

#[test]
fn settings_round_trip() {
    let settings = Settings::memory_only();
    settings.put_bool(keys::MUTE, true);
    assert!(settings.get_bool(keys::MUTE));
    settings.put_int(keys::WINDOW_WIDTH, 1234);
    assert_eq!(settings.get_int(keys::WINDOW_WIDTH), 1234);
    settings.put_double(keys::VOLUME, 0.75);
    assert!((settings.get_double(keys::VOLUME) - 0.75).abs() < f64::EPSILON);
    settings.put_string(keys::HWDEC, "vaapi");
    assert_eq!(settings.get_string(keys::HWDEC), "vaapi");
}

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
fn converter_settings_keys_have_defaults() {
    let defaults = Settings::defaults();
    for key in [
        "last-profile",
        "output-dir",
        "gpu-device",
        "gpu-auto-migrated",
        "overwrite-existing",
        "parallel-jobs",
        "notify-done",
        "delete-source",
        "prevent-sleep",
        "shutdown-after",
        "saved-profile",
        "conv-last-dir",
        "conv-segment-mode",
        "conv-nr-enabled",
        "conv-nr-lookahead",
        "conv-nr-strength",
        "conv-nr-model",
        "conv-nr-voice-recovery",
        "conv-gate-enabled",
        "conv-gate-intensity",
        "conv-hpf-enabled",
        "conv-hpf-freq",
        "conv-normalize",
        "conv-compressor-enabled",
        "conv-compressor-intensity",
        "conv-eq-enabled",
        "conv-eq-preset",
        "conv-eq-bands",
        "audio-conv-format",
        "audio-conv-bitrate",
        "audio-conv-channels",
        "audio-conv-cut-mode",
        "audio-conv-cut-output",
        "audio-conv-volume",
        "audio-conv-speed",
    ] {
        assert!(defaults.contains_key(key), "missing default for {key}");
    }
}

#[test]
fn invalid_values_do_not_mutate_state() {
    let settings = Settings::memory_only();
    assert!(!settings.set_double("volume", f64::NAN));
    assert!(!settings.set_string("title", "one\ntwo"));
    assert!(!settings.set_string("volume", "not-a-number"));
    assert!(settings.set_double("volume", 0.0));
    assert!(settings.try_flush().is_err()); // never use /tmp as HOME fallback
}

#[test]
fn shared_handles_and_external_conflict_are_not_lost_updates() {
    use big_os_kit::storage::CommitOutcome;
    let path = std::env::temp_dir().join(format!("settings-review-{}.conf", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let a = Settings::from_path(path.clone());
    let b = Settings::from_path(path.clone());
    a.put_double(keys::VOLUME, 0.0);
    assert_eq!(b.get_double(keys::VOLUME), 0.0);
    assert!(matches!(b.try_flush().unwrap(), CommitOutcome::Durable));
    std::fs::write(&path, b"volume=21\n").unwrap();
    a.put_double(keys::VOLUME, 30.0);
    assert!(a.try_flush().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"volume=21\n");
    assert_eq!(a.get_double(keys::VOLUME), 30.0);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(
        path.with_file_name(format!("settings-review-{}.conf.lock", std::process::id())),
    );
}

#[test]
fn memory_only_cannot_confirm_even_an_unedited_save() {
    let settings = Settings::memory_only();
    assert!(settings.try_flush().is_err());
    assert_eq!(settings.save_status().phase, SavePhase::Failed);
}
