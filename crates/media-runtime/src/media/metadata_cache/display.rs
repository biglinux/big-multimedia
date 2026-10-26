use crate::media::playlist::VideoMeta;
use big_media_components::time::duration_hms as format_duration_hms;

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
        let count = u64::from(meta.audio_tracks);
        parts.push(
            crate::i18n::ni18n("{} audio track", "{} audio tracks", count)
                .replace("{}", &count.to_string()),
        );
    }
    if meta.subtitle_tracks > 0 {
        let count = u64::from(meta.subtitle_tracks);
        parts.push(
            crate::i18n::ni18n("{} subtitle", "{} subtitles", count)
                .replace("{}", &count.to_string()),
        );
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
