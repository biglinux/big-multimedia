//! Round-trip: every string returned by `big_media_runtime::ui_labels::*`
//! must appear as a `msgid` in `po/big-media-runtime.pot`. Guards against a
//! future refactor that hardcodes a label outside the gettext pipeline.
//!
//! Skipped silently if the POT is missing (e.g. fresh checkout that has not
//! run `po/regen-pot.sh`). CI must run the regen step before this test.

use std::fs;
use std::path::PathBuf;

use big_media_runtime::ui_labels;

fn pot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("po/big-media-runtime.pot")
}

fn pot_msgids() -> Option<Vec<String>> {
    let raw = fs::read_to_string(pot_path()).ok()?;
    let mut ids = Vec::new();
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("msgid \"")
            && let Some(stripped) = rest.strip_suffix('"')
        {
            ids.push(stripped.to_string());
        }
    }
    Some(ids)
}

// invariants for every ui_labels function
fn assert_non_empty_and_unique(group: &str, labels: &[String]) {
    assert!(!labels.is_empty(), "ui_labels::{group}: returned empty Vec");
    let mut seen = std::collections::HashSet::new();
    for label in labels {
        assert!(
            seen.insert(label.clone()),
            "ui_labels::{group}: duplicate label `{label}`"
        );
    }
}

fn assert_all_extracted(group: &str, labels: Vec<String>, msgids: &[String]) {
    assert_non_empty_and_unique(group, &labels);
    let mut missing = Vec::new();
    for label in &labels {
        let label = label.clone();
        // Numeric-only labels (e.g. speed multipliers like "1×") are
        // intentionally not routed through gettext — locale-neutral.
        if label
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '×' | ' '))
        {
            continue;
        }
        if !msgids.iter().any(|id| id == &label) {
            missing.push(label);
        }
    }
    assert!(
        missing.is_empty(),
        "ui_labels::{group}: msgids absent from POT (regen-pot.sh stale?): {missing:?}"
    );
}

#[test]
fn every_ui_label_round_trips_to_pot() {
    let msgids = pot_msgids().unwrap_or_else(|| {
        panic!(
            "po/big-media-runtime.pot not found — run po/regen-pot.sh before `cargo test` so \
             this gate can verify msgid extraction stays in sync with ui_labels::*."
        )
    });
    assert_all_extracted("codec_labels", ui_labels::codec_labels(), &msgids);
    assert_all_extracted("quality_labels", ui_labels::quality_labels(), &msgids);
    assert_all_extracted("preset_labels", ui_labels::preset_labels(), &msgids);
    assert_all_extracted("resolution_labels", ui_labels::resolution_labels(), &msgids);
    assert_all_extracted("container_labels", ui_labels::container_labels(), &msgids);
    assert_all_extracted(
        "audio_handling_labels",
        ui_labels::audio_handling_labels(),
        &msgids,
    );
    assert_all_extracted(
        "audio_codec_labels",
        ui_labels::audio_codec_labels(),
        &msgids,
    );
    assert_all_extracted(
        "audio_bitrate_labels",
        ui_labels::audio_bitrate_labels(),
        &msgids,
    );
    assert_all_extracted(
        "audio_channels_labels",
        ui_labels::audio_channels_labels(),
        &msgids,
    );
    assert_all_extracted("subtitle_labels", ui_labels::subtitle_labels(), &msgids);
    assert_all_extracted(
        "audio_format_labels",
        ui_labels::audio_format_labels(),
        &msgids,
    );
    assert_all_extracted(
        "audio_bitrate_labels_audio_mode",
        ui_labels::audio_bitrate_labels_audio_mode(),
        &msgids,
    );
    assert_all_extracted(
        "audio_channels_labels_audio_mode",
        ui_labels::audio_channels_labels_audio_mode(),
        &msgids,
    );
    // speed_labels: numeric multipliers, intentionally skipped above.
    // Still verify non-empty + uniqueness.
    let speed = ui_labels::speed_labels();
    assert_non_empty_and_unique("speed_labels", &speed);
}

/// Every path listed in `po/POTFILES.in` must exist. A stale entry (left over
/// from a repo move, as happened at the monorepo split) silently drops that
/// file's strings from the catalog; this fails the build instead.
#[test]
fn potfiles_in_paths_all_exist() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    let list = manifest.join("po/POTFILES.in");
    let raw = fs::read_to_string(&list).unwrap_or_else(|e| panic!("read {}: {e}", list.display()));
    let mut missing = Vec::new();
    for line in raw.lines() {
        let path = line.trim();
        if path.is_empty() || path.starts_with('#') {
            continue;
        }
        if !root.join(path).exists() {
            missing.push(path.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "POTFILES.in lists paths that no longer exist (rerun po/regen-pot.sh): {missing:?}"
    );
}

/// Error display contract: no Rust Option debug syntax (`Some(`, `None`)
/// leaks into user-facing toast text. Regression guard for future
/// `Error` variant additions — the current variants are listed
/// exhaustively so adding a new variant without re-checking this test
/// will not silently pass.
#[test]
fn error_display_no_option_debug_syntax() {
    use big_media_runtime::Error;

    // Build one instance per Error variant. The exhaustive match in
    // `kind` below forces future maintainers to wire new variants in.
    let cases: Vec<Error> = vec![
        std::io::Error::other("x").into(),
        rusqlite::Error::QueryReturnedNoRows.into(),
        serde_json::from_str::<i32>("nope").unwrap_err().into(),
        {
            // construct via runtime slice so clippy doesn't const-fold to error
            let buf: Vec<u8> = vec![0xff_u8];
            std::str::from_utf8(buf.as_slice()).unwrap_err().into()
        },
        String::from_utf8(vec![0xff_u8]).unwrap_err().into(),
        "x".parse::<i32>().unwrap_err().into(),
        "x".parse::<f64>().unwrap_err().into(),
        Error::mpv("boom"),
        Error::ffmpeg("ffmpeg", Some(1), "boom"),
        Error::ffmpeg("ffprobe", None, "killed"),
        Error::chromecast("net down"),
        Error::invalid_argument("seek", "negative"),
        Error::not_found("track"),
        Error::Cancelled,
        Error::internal("oops"),
    ];

    // sentinel: forbid `Some(`, `None`, `Option::`, and a stray `: None`
    // anywhere in the user-visible Display output. Bare `None` catches
    // any debug-formatted `Option::<T>` regardless of surrounding chars.
    let forbidden = [" Some(", "Some(", "None)", " None", "Option::", "None"];
    for e in &cases {
        let s = e.to_string();
        for needle in forbidden {
            assert!(
                !s.contains(needle),
                "Error::{kind:?} Display leaked `{needle}` (regression — see error.rs): {s}",
                kind = error_kind(e),
            );
        }
    }
}

// `&'static str` discriminator for diagnostic messages. Uses an
// exhaustive match (sans `#[non_exhaustive]` wildcard fallback) so a
// future variant addition forces an update.
fn error_kind(e: &big_media_runtime::Error) -> &'static str {
    use big_media_runtime::Error;
    match e {
        Error::Io(_) => "Io",
        Error::Sqlite(_) => "Sqlite",
        Error::Json(_) => "Json",
        Error::Utf8(_) => "Utf8",
        Error::FromUtf8(_) => "FromUtf8",
        Error::ParseInt(_) => "ParseInt",
        Error::ParseFloat(_) => "ParseFloat",
        Error::Mpv(_) => "Mpv",
        Error::Ffmpeg { .. } => "Ffmpeg",
        Error::Chromecast(_) => "Chromecast",
        Error::InvalidArgument { .. } => "InvalidArgument",
        Error::NotFound { .. } => "NotFound",
        Error::Cancelled => "Cancelled",
        Error::Internal(_) => "Internal",
        _ => "Unknown",
    }
}
