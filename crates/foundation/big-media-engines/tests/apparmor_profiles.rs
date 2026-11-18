//! The players' AppArmor profiles are rendered from one template and the media
//! types in `media_files`, so a type a player opens is one its profile reads.
//! `BLESS_APPARMOR=1` rewrites the profiles after a template or list change.

use std::path::{Path, PathBuf};

use big_media_engines::media_files::{
    AUDIO_EXTENSIONS, IMAGE_EXTENSIONS, LYRICS_EXTENSION, PLAYLIST_EXTENSIONS, SUBTITLE_EXTENSIONS,
    VIDEO_EXTENSIONS,
};

const PLAYERS: [&str; 2] = ["big-audio-player", "big-video-player"];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// `m3u8` as `[mM]3[uU]8`: AppArmor globs match case, and media names come in both.
fn any_case(extension: &str) -> String {
    extension
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase())
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn variable(name: &str, extensions: &[&str]) -> String {
    let patterns: Vec<String> = extensions.iter().map(|ext| any_case(ext)).collect();
    // AppArmor refuses a one-item `{}` alternation.
    match patterns.as_slice() {
        [single] => format!("@{{{name}}} = {single}"),
        _ => format!("@{{{name}}} = {{{}}}", patterns.join(",")),
    }
}

fn media_types() -> String {
    let media: Vec<&str> = VIDEO_EXTENSIONS
        .iter()
        .chain(AUDIO_EXTENSIONS)
        .copied()
        .collect();
    let companion: Vec<&str> = SUBTITLE_EXTENSIONS
        .iter()
        .chain(IMAGE_EXTENSIONS)
        .copied()
        .chain([LYRICS_EXTENSION])
        .collect();
    [
        variable("media_ext", &media),
        variable("companion_ext", &companion),
        variable("playlist_ext", PLAYLIST_EXTENSIONS),
        variable("screenshot_ext", &["png"]),
    ]
    .join("\n")
}

#[test]
fn profiles_follow_template_and_types() {
    let root = workspace();
    let template =
        std::fs::read_to_string(root.join("crates/media-runtime/data/apparmor/player.apparmor.in"))
            .expect("profile template");
    for app in PLAYERS {
        let path = root.join(format!("crates/{app}/resources/apparmor/{app}"));
        let expected = template
            .replace("@MEDIA_TYPES@", &media_types())
            .replace("@APP@", app);
        if std::env::var_os("BLESS_APPARMOR").is_some() {
            std::fs::create_dir_all(path.parent().expect("profile directory"))
                .expect("create profile directory");
            std::fs::write(&path, &expected).expect("write profile");
            continue;
        }
        let shipped = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            shipped == expected,
            "{} is out of date: run BLESS_APPARMOR=1 cargo test -p big-media-engines --test apparmor_profiles",
            path.display()
        );
    }
}
