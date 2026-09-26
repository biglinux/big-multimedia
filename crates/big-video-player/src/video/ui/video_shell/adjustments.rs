//! "Active adjustments": every change from the normal picture and sound,
//! named in words. A small pill beside the header's settings button says
//! one is on (like the "No sound" pill), without covering the video; the
//! settings panel lists them, each with its ×, and "Back to normal".
//!
//! mpv (and, for the equalizer, `Settings`) stays the source of truth: the
//! list is re-read from it whenever the controls appear or a dialog closes.

use super::*;
use crate::video::ui::media_adjust::{CropMargins, source_colour_names};

/// `vf` labels the chips track elsewhere (flip, source colours).
const OWNED_VF_LABELS: [&str; 2] = ["big-flip", "source-colours"];
const FLAT_EQ: &str = "0.0;0.0;0.0;0.0;0.0;0.0;0.0;0.0;0.0;0.0";

/// One change from normal, holding the value it replaced so undoing the
/// reset can put it back.
#[derive(Debug, Clone, PartialEq)]
pub enum Adjustment {
    SourceColours(usize),
    /// Contrast, brightness, gamma, saturation, hue.
    Picture([i64; 5]),
    Zoom {
        zoom: f64,
        panscan: f64,
    },
    Rotation(i64),
    Flip {
        horizontal: bool,
        vertical: bool,
    },
    Crop {
        margins: CropMargins,
        crop: String,
    },
    Speed(f64),
    AudioDelay(f64),
    SubtitleDelay(f64),
    Equalizer {
        bands: String,
        preset: String,
    },
    /// `vf` entries from the filters dialog, as `get_vf` lists them.
    VideoEffects(Vec<String>),
    /// The saved upscaler, neural upscaler and Anime4K preset.
    ImageEnhancement([String; 3]),
    /// Seconds left; a timer is a plan, not a change, so "Back to normal"
    /// leaves it.
    SleepTimer(u32),
}

fn changed(value: f64) -> bool {
    value.abs() > 1e-3
}

/// Every adjustment currently away from normal, in the order the chips show.
pub(super) fn detect(
    mpv: &MpvBackend,
    settings: &Settings,
    transform: &media_adjust::VideoTransformState,
) -> Vec<Adjustment> {
    let mut found = Vec::new();
    let colours = transform.source_colours_index();
    if colours != 0 {
        found.push(Adjustment::SourceColours(colours));
    }
    let picture = [
        mpv.get_contrast(),
        mpv.get_brightness(),
        mpv.get_gamma(),
        mpv.get_saturation(),
        mpv.get_hue(),
    ];
    if picture.iter().any(|v| *v != 0) {
        found.push(Adjustment::Picture(picture));
    }
    let (zoom, panscan) = (mpv.get_video_zoom(), mpv.get_panscan());
    if changed(zoom) || changed(panscan) {
        found.push(Adjustment::Zoom { zoom, panscan });
    }
    let rotation = mpv.get_video_rotate();
    if rotation != 0 {
        found.push(Adjustment::Rotation(rotation));
    }
    if transform.hflip() || transform.vflip() {
        found.push(Adjustment::Flip {
            horizontal: transform.hflip(),
            vertical: transform.vflip(),
        });
    }
    let crop = mpv.get_property_string("video-crop");
    if !crop.is_empty() && crop != "0x0+0+0" {
        found.push(Adjustment::Crop {
            margins: transform.crop_margins(),
            crop,
        });
    }
    let speed = mpv.get_speed();
    if changed(speed - 1.0) {
        found.push(Adjustment::Speed(speed));
    }
    let audio_delay = mpv.get_property_f64("audio-delay");
    if changed(audio_delay) {
        found.push(Adjustment::AudioDelay(audio_delay));
    }
    let subtitle_delay = mpv.get_property_f64("sub-delay");
    if changed(subtitle_delay) {
        found.push(Adjustment::SubtitleDelay(subtitle_delay));
    }
    let bands = settings.get_string(keys::EQ_BANDS);
    if crate::shared::audio_effects::parse_eq_bands(&bands)
        .iter()
        .any(|gain| changed(*gain))
    {
        found.push(Adjustment::Equalizer {
            bands,
            preset: settings.get_string(keys::EQ_PRESET),
        });
    }
    let effects: Vec<String> = vf_entries(&mpv.get_vf())
        .into_iter()
        .filter(|entry| vf_label(entry).is_some_and(|label| !OWNED_VF_LABELS.contains(&label)))
        .collect();
    if !effects.is_empty() {
        found.push(Adjustment::VideoEffects(effects));
    }
    let enhancements = crate::video::ui::filters::saved_enhancements(settings);
    if enhancements.iter().any(|choice| !choice.is_empty()) {
        found.push(Adjustment::ImageEnhancement(enhancements));
    }
    found
}

/// Split mpv's `vf` list at its top-level commas. mpv quotes a value holding
/// commas as `%LEN%text`, so those bytes are skipped whole.
fn vf_entries(vf: &str) -> Vec<String> {
    let bytes = vf.as_bytes();
    let mut entries = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let digits = bytes[i + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                let close = i + 1 + digits;
                if digits > 0 && bytes.get(close) == Some(&b'%') {
                    let len: usize = vf[i + 1..close].parse().unwrap_or(0);
                    i = (close + 1 + len).min(bytes.len());
                    continue;
                }
            }
            b',' => {
                entries.push(vf[start..i].to_owned());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < bytes.len() {
        entries.push(vf[start..].to_owned());
    }
    entries
}

fn vf_label(entry: &str) -> Option<&str> {
    entry.strip_prefix('@')?.split(':').next()
}

/// An equalizer preset in words that say what it does to the sound.
pub(super) fn sound_type_name(preset: &str) -> String {
    match preset {
        "Flat" => i18n("Normal"),
        "Bass Boost" => i18n("More bass"),
        "Treble Boost" => i18n("More treble"),
        "Vocal" => i18n("Voices"),
        other => i18n(other),
    }
}

fn seconds(value: f64) -> String {
    format!("{:.1}", value.abs())
}

impl Adjustment {
    /// What the chip says, in words for people who never opened the dialog.
    pub(super) fn label(&self) -> String {
        match self {
            Self::SourceColours(index) => i18n("Video colour: {name}").replace(
                "{name}",
                source_colour_names().get(*index).map_or("", String::as_str),
            ),
            Self::Picture(_) => i18n("Picture adjusted"),
            Self::Zoom { zoom, .. } if changed(*zoom) => i18n("Zoom {percent}%")
                .replace("{percent}", &format!("{:.0}", 2_f64.powf(*zoom) * 100.0)),
            Self::Zoom { .. } => i18n("Picture enlarged to fill"),
            Self::Rotation(degrees) => {
                i18n("Rotated {degrees}°").replace("{degrees}", &degrees.to_string())
            }
            Self::Flip { .. } => i18n("Mirrored"),
            Self::Crop { .. } => i18n("Edges cut off"),
            Self::Speed(speed) => i18n("Speed {speed}×").replace("{speed}", &format!("{speed:.2}")),
            Self::AudioDelay(delay) if *delay > 0.0 => {
                i18n("Sound {seconds} s late").replace("{seconds}", &seconds(*delay))
            }
            Self::AudioDelay(delay) => {
                i18n("Sound {seconds} s early").replace("{seconds}", &seconds(*delay))
            }
            Self::SubtitleDelay(delay) if *delay > 0.0 => {
                i18n("Subtitles {seconds} s late").replace("{seconds}", &seconds(*delay))
            }
            Self::SubtitleDelay(delay) => {
                i18n("Subtitles {seconds} s early").replace("{seconds}", &seconds(*delay))
            }
            Self::Equalizer { bands, .. } => i18n("Type of sound: {name}").replace(
                "{name}",
                &crate::shared::audio_effects::equalizer_preset_matching(bands)
                    .and_then(|index| {
                        crate::shared::audio_effects::equalizer_preset_names()
                            .get(index)
                            .copied()
                    })
                    .map_or_else(|| i18n("by hand"), sound_type_name),
            ),
            Self::VideoEffects(_) => i18n("Video effects on"),
            Self::ImageEnhancement(_) => i18n("Picture enhanced"),
            Self::SleepTimer(secs) => {
                let at = glib::DateTime::now_local()
                    .and_then(|now| now.add_seconds(f64::from(*secs)))
                    .and_then(|at| at.format("%H:%M"))
                    .map(|at| at.to_string())
                    .unwrap_or_default();
                i18n("Stops at {time}").replace("{time}", &at)
            }
        }
    }

    /// The `win.*` action that opens the section this chip belongs to.
    pub(super) fn section_action(&self) -> Option<&'static str> {
        match self {
            Self::SourceColours(_) => Some("win.show-hdr"),
            Self::Picture(_) => Some("win.show-image-adjust"),
            Self::Zoom { .. } | Self::Rotation(_) | Self::Flip { .. } | Self::Crop { .. } => {
                Some("win.show-framing")
            }
            // The controls bar is where these are set.
            Self::Speed(_) | Self::SubtitleDelay(_) => None,
            Self::AudioDelay(_) | Self::Equalizer { .. } => Some("win.show-sound"),
            Self::VideoEffects(_) | Self::ImageEnhancement(_) => Some("win.filters"),
            Self::SleepTimer(_) => Some("win.sleep-timer"),
        }
    }

    /// Put this setting back to normal (`normal`) or to the value it held.
    /// The sleep timer is the shell's; it is cancelled there.
    pub(super) fn apply(
        &self,
        normal: bool,
        mpv: &MpvBackend,
        settings: &Settings,
        transform: &media_adjust::VideoTransformState,
    ) {
        match self {
            Self::SourceColours(index) => {
                transform.select_source_colours(mpv, if normal { 0 } else { *index });
            }
            Self::Picture(values) => {
                let [contrast, brightness, gamma, saturation, hue] =
                    if normal { [0; 5] } else { *values };
                mpv.set_contrast(contrast);
                mpv.set_brightness(brightness);
                mpv.set_gamma(gamma);
                mpv.set_saturation(saturation);
                mpv.set_hue(hue);
            }
            Self::Zoom { zoom, panscan } => {
                mpv.set_video_zoom(if normal { 0.0 } else { *zoom });
                mpv.set_panscan(if normal { 0.0 } else { *panscan });
            }
            Self::Rotation(degrees) => mpv.set_video_rotate(if normal { 0 } else { *degrees }),
            Self::Flip {
                horizontal,
                vertical,
            } => transform.set_flip(mpv, !normal && *horizontal, !normal && *vertical),
            Self::Crop { margins, crop } => {
                transform.set_crop_margins(if normal {
                    CropMargins::default()
                } else {
                    *margins
                });
                mpv.set_video_crop(if normal { "" } else { crop });
            }
            Self::Speed(speed) => mpv.set_speed(if normal { 1.0 } else { *speed }),
            Self::AudioDelay(delay) => mpv.set_audio_delay(if normal { 0.0 } else { *delay }),
            Self::SubtitleDelay(delay) => mpv.set_sub_delay(if normal { 0.0 } else { *delay }),
            Self::Equalizer { bands, preset } => {
                let _ = settings.put_string(keys::EQ_BANDS, if normal { FLAT_EQ } else { bands });
                let _ = settings.put_string(keys::EQ_PRESET, if normal { "flat" } else { preset });
                crate::shared::audio_effects::apply_voice_change(settings, mpv);
            }
            Self::VideoEffects(entries) => {
                for entry in entries {
                    if normal {
                        if let Some(label) = vf_label(entry) {
                            mpv.vf_remove(label);
                        }
                    } else {
                        mpv.vf_add_entry(entry);
                    }
                }
            }
            Self::ImageEnhancement(choices) => {
                let keys = [
                    keys::BUILTIN_UPSCALER,
                    keys::NEURAL_UPSCALER,
                    keys::ANIME4K_PRESET,
                ];
                for (key, choice) in keys.into_iter().zip(choices) {
                    let _ = settings.put_string(key, if normal { "" } else { choice });
                }
                crate::video::ui::filters::apply_saved_enhancements(mpv, settings);
            }
            Self::SleepTimer(_) => {}
        }
    }
}

impl VideoShell {
    /// Re-read the adjustments and update the header pill.
    pub(super) fn refresh_adjustments(&mut self, _sender: &ComponentSender<Self>) {
        let Some(mpv) = self.mpv.borrow().clone() else {
            return;
        };
        let mut found = detect(&mpv, &self.settings, &self.video_transform);
        if self.sleep_timer_is_armed() {
            found.push(Adjustment::SleepTimer(self.sleep_timer_remaining_secs()));
        }
        let names: Vec<String> = found.iter().map(Adjustment::label).collect();
        let text = match names.as_slice() {
            [only] => only.clone(),
            many => ni18n("{n} change", "{n} changes", many.len() as u64)
                .replace("{n}", &many.len().to_string()),
        };
        let tooltip = format!("{}\n{}", names.join("\n"), i18n("Click to see or undo"));
        self.adjustments_pill.set_label(&format!("● {text}"));
        self.adjustments_pill.set_tooltip_text(Some(&tooltip));
        self.adjustments_pill
            .update_property(&[gtk::accessible::Property::Label(&tooltip)]);
        self.adjustments_pill.set_visible(!found.is_empty());
        if self.video_transform.source_colours_index() != 0 {
            self.hdr_hint.set_visible(false);
        }
        self.adjustments = found;
    }

    pub(super) fn apply_adjustments(&self, adjustments: &[Adjustment], normal: bool) {
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            for adjustment in adjustments {
                adjustment.apply(normal, mpv, &self.settings, &self.video_transform);
            }
        }
    }

    /// Put `adjustments` back to normal at once, no question asked: the
    /// panel offers "Undo" until it closes.
    pub(super) fn reset_adjustments(
        &mut self,
        adjustments: Vec<Adjustment>,
        sender: &ComponentSender<Self>,
    ) {
        if adjustments.is_empty() {
            return;
        }
        self.apply_adjustments(&adjustments, true);
        self.last_reset = Some(adjustments);
        self.refresh_adjustments(sender);
        self.refresh_panel_home(sender);
    }

    pub(super) fn restore_adjustments(
        &mut self,
        adjustments: &[Adjustment],
        sender: &ComponentSender<Self>,
    ) {
        self.apply_adjustments(adjustments, false);
        self.last_reset = None;
        self.refresh_adjustments(sender);
        self.refresh_panel_home(sender);
    }

    /// HDR that does not say so plays grey and washed out, and nothing on
    /// screen tells why: 10-bit 4K frames with no colour description are
    /// that case. Offer the fix in a header pill; apply it only on request.
    pub(super) fn suggest_hdr(&self, path: &str) {
        let Some(mpv) = self.mpv.borrow().clone() else {
            return;
        };
        let undescribed_hdr = mpv.get_current_file().as_deref() == Some(path)
            && self.video_transform.source_colours_index() == 0
            // GPU decoding names the surface ("cuda") and puts the depth here.
            && ["pixelformat", "hw-pixelformat"].iter().any(|name| {
                let format = mpv.get_property_string(&format!("video-dec-params/{name}"));
                ["p10", "p12", "p010", "p016"]
                    .iter()
                    .any(|depth| format.contains(depth))
            })
            && mpv.get_property_string("video-dec-params/gamma") == "auto"
            && mpv.get_property_string("video-dec-params/primaries") == "auto"
            && mpv.get_property_f64("video-dec-params/w") >= 3840.0;
        self.hdr_hint.set_visible(undescribed_hdr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vf_entries_keep_quoted_commas_inside_one_entry() {
        let vf = "@denoise:lavfi=graph=%13%hqdn3d=4,eq=1,@big-flip:lavfi=graph=hflip,\
                  @source-colours:format=gamma=pq:primaries=%7%bt.2020";
        assert_eq!(
            vf_entries(vf),
            [
                "@denoise:lavfi=graph=%13%hqdn3d=4,eq=1",
                "@big-flip:lavfi=graph=hflip",
                "@source-colours:format=gamma=pq:primaries=%7%bt.2020",
            ]
        );
        assert_eq!(vf_label("@big-flip:lavfi=graph=hflip"), Some("big-flip"));
        assert_eq!(vf_label("format=gamma=pq"), None);
    }

    #[test]
    fn vf_entries_of_empty_list_is_empty() {
        assert!(vf_entries("").is_empty());
    }
}
