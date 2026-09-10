//! Output filename + container resolver. `auto` container falls through
//! profile.video.container then derives by codec (h264/h265→mp4,
//! vp9→webm, prores→mov, otherwise mkv). Audio-only path picks the
//! codec extension when re-encoding, source ext when copying. The
//! handwritten `chrono_date` keeps `{date}` token expansion working
//! without pulling chrono into the dep tree.

use std::path::Path;

use super::Profile;

#[must_use]
pub fn resolve_container(profile: &Profile) -> &str {
    let container = if profile.output.container == "auto" {
        &profile.video.container
    } else {
        &profile.output.container
    };

    if container == "auto" {
        match profile.video.codec.as_str() {
            "h264" | "h265" => "mp4",
            "vp9" => "webm",
            "prores" => "mov",
            _ => "mkv",
        }
    } else {
        container
    }
}

// Tokens: {name} = input stem, {ext}, {codec}, {date}, {quality}.
#[must_use]
pub fn resolve_output_name(profile: &Profile, input: &Path) -> String {
    let stem = input
        .file_stem()
        .map_or("output", |s| s.to_str().unwrap_or("output"));
    let ext = resolve_container(profile);
    let codec = &profile.video.codec;
    let quality = &profile.video.quality;

    replace_output_name_tokens(
        &profile.output.naming,
        stem,
        ext,
        codec,
        quality,
        chrono_date,
    )
}

fn replace_output_name_tokens(
    naming_template: &str,
    stem: &str,
    ext: &str,
    codec: &str,
    quality: &str,
    date_text: impl FnOnce() -> String,
) -> String {
    let date = naming_template.contains("{date}").then(date_text);
    naming_template
        .replace("{name}", stem)
        .replace("{ext}", ext)
        .replace("{codec}", codec)
        .replace("{date}", date.as_deref().unwrap_or(""))
        .replace("{quality}", quality)
}

#[must_use]
pub fn resolve_audio_output_name(profile: &Profile, input: &Path) -> String {
    let stem = input
        .file_stem()
        .map_or("output", |s| s.to_str().unwrap_or("output"));
    let source_ext = input.extension().and_then(|e| e.to_str()).unwrap_or("m4a");
    let ext = if !profile.output.container.is_empty() && profile.output.container != "auto" {
        profile.output.container.as_str()
    } else if profile.audio.handling != "copy" {
        match profile.audio.codec.as_str() {
            "aac" => "aac",
            "opus" => "opus",
            "mp3" => "mp3",
            "ac3" => "ac3",
            "flac" => "flac",
            "pcm" => "wav",
            "vorbis" => "ogg",
            _ => "m4a",
        }
    } else {
        source_ext
    };

    replace_output_name_tokens(
        &profile.output.naming,
        stem,
        ext,
        &profile.audio.codec,
        &profile.video.quality,
        chrono_date,
    )
}

fn chrono_date() -> String {
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86_400;
    let mut year = 1970_u64;
    let mut remaining = days;

    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0;
    for (idx, &days_in_month) in month_days.iter().enumerate() {
        if remaining < days_in_month {
            month = idx;
            break;
        }
        remaining -= days_in_month;
    }

    format!("{year}-{:02}-{:02}", month + 1, remaining + 1)
}

pub(super) fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}
