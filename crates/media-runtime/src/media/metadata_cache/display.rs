use crate::media::playlist::VideoMeta;
use crate::shared::ui_helpers::format_duration_hms;

#[must_use]
pub fn meta_summary(meta: &VideoMeta) -> String {
    let mut parts = Vec::new();
    if let Some(dur) = meta.duration_secs
        && dur > 0.0
    {
        parts.push(format_duration_hms(dur));
    }
    if let Some(resolution_text) = &meta.resolution {
        parts.push(resolution_text.clone());
    }
    if let Some(codec) = &meta.codec {
        parts.push(codec.clone());
    }
    if meta.audio_tracks > 0 {
        parts.push(format!(
            "{} audio{}",
            meta.audio_tracks,
            if meta.audio_tracks > 1 { "s" } else { "" }
        ));
    }
    if meta.subtitle_tracks > 0 {
        parts.push(format!(
            "{} sub{}",
            meta.subtitle_tracks,
            if meta.subtitle_tracks > 1 { "s" } else { "" }
        ));
    }
    parts.join(" · ")
}

#[must_use]
pub fn resolution_label(meta: &VideoMeta) -> Option<String> {
    let resolution_text = meta.resolution.as_ref()?;
    let parts: Vec<&str> = resolution_text.split(['×', 'x']).collect();
    if parts.len() != 2 {
        return None;
    }
    let h: i64 = parts[1].trim().parse().ok()?;
    Some(match h {
        2160.. => "4K".into(),
        1440..=2159 => "1440p".into(),
        1080..=1439 => "1080p".into(),
        720..=1079 => "720p".into(),
        480..=719 => "480p".into(),
        _ => format!("{h}p"),
    })
}
