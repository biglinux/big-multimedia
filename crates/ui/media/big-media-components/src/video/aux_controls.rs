// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Auxiliary video controls: speed, volume, audio track, subtitle track.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use big_relm4_components::feedback::tooltip;
use big_relm4_components::layout::hamburger_menu::attach_popover_to_button;
use relm4::gtk;

/// Translated labels rendered next to auxiliary video transport buttons.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoAuxControlLabels {
    pub playback_speed: String,
    pub volume: String,
    pub audio_track: String,
    pub subtitle_track: String,
    pub no_tracks_loaded: String,
    pub no_subtitles: String,
}

impl BigVideoAuxControlLabels {
    /// Hard-coded English defaults useful for unit tests and previews.
    #[must_use]
    pub fn english() -> Self {
        Self {
            playback_speed: "Playback speed".into(),
            volume: "Volume".into(),
            audio_track: "Audio track".into(),
            subtitle_track: "Subtitle track".into(),
            no_tracks_loaded: "No tracks loaded".into(),
            no_subtitles: "No subtitles".into(),
        }
    }
}

/// Builder spec for [`BigVideoAuxControls`].
///
/// Use the chained `default_speed_index` and `volume_range` builders to
/// adjust the defaults before passing to [`BigVideoAuxControls::new`].
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoAuxControlSpec {
    pub labels: BigVideoAuxControlLabels,
    pub speed_labels: Vec<String>,
    pub default_speed_index: usize,
    pub volume_max: u32,
    pub volume_default: u32,
}

impl BigVideoAuxControlSpec {
    /// Build a spec from translated labels and a list of speed presets.
    #[must_use]
    pub fn new(labels: BigVideoAuxControlLabels, speed_labels: &[&str]) -> Self {
        Self {
            labels,
            speed_labels: speed_labels
                .iter()
                .map(|label| (*label).to_string())
                .collect(),
            default_speed_index: 0,
            volume_max: 150,
            volume_default: 100,
        }
    }

    /// Override the initially selected speed index.
    #[must_use]
    pub fn default_speed_index(mut self, index: usize) -> Self {
        self.default_speed_index = index;
        self
    }

    /// Override the volume scale range.
    #[must_use]
    pub fn volume_range(mut self, default_value: u32, max_value: u32) -> Self {
        self.volume_default = default_value;
        self.volume_max = max_value;
        self
    }

    /// Return a resolved view summarising clamped defaults.
    #[must_use]
    pub fn resolved(&self) -> BigVideoAuxControlResolved {
        BigVideoAuxControlResolved {
            speed_count: self.speed_labels.len(),
            default_speed_index: self
                .default_speed_index
                .min(self.speed_labels.len().saturating_sub(1)),
            volume_default: self.volume_default.min(self.volume_max),
            volume_max: self.volume_max,
        }
    }
}

/// Resolved view of a [`BigVideoAuxControlSpec`] for tests and validators.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoAuxControlResolved {
    pub speed_count: usize,
    pub default_speed_index: usize,
    pub volume_default: u32,
    pub volume_max: u32,
}

/// Speed/volume/audio-track/subtitle-track popovers for a video transport.
#[derive(Debug, Clone)]
pub struct BigVideoAuxControls {
    speed_button: gtk::Button,
    speed_box: gtk::Box,
    volume_button: gtk::Button,
    volume_scale: gtk::Scale,
    audio_track_button: gtk::Button,
    subtitle_track_button: gtk::Button,
    audio_track_box: gtk::Box,
    subtitle_track_box: gtk::Box,
}

impl BigVideoAuxControls {
    /// Build all four popover-backed buttons from `spec`.
    #[must_use]
    pub fn new(spec: BigVideoAuxControlSpec) -> Self {
        let labels = spec.labels.clone();
        let resolved = spec.resolved();

        let speed_popover = gtk::Popover::new();
        let speed_box = popover_box();
        let speed_group = Rc::new(RefCell::new(None::<gtk::CheckButton>));
        for (idx, label) in spec.speed_labels.iter().enumerate() {
            let check = gtk::CheckButton::builder().label(label).build();
            let current = speed_group.borrow().clone();
            if let Some(ref group) = current {
                check.set_group(Some(group));
            } else {
                *speed_group.borrow_mut() = Some(check.clone());
            }
            if idx == resolved.default_speed_index {
                check.set_active(true);
            }
            speed_box.append(&check);
        }
        speed_popover.set_child(Some(&speed_box));
        let speed_label = spec
            .speed_labels
            .get(resolved.default_speed_index)
            .cloned()
            .unwrap_or_default();
        let speed_button = gtk::Button::builder()
            .label(&speed_label)
            .css_classes(["flat"])
            .build();
        tooltip::set(&speed_button, &labels.playback_speed);
        speed_button.update_property(&[gtk::accessible::Property::Label(&labels.playback_speed)]);
        attach_popover_to_button(&speed_button, &speed_popover);

        let volume_popover = gtk::Popover::new();
        let volume_box = popover_box();
        let volume_scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Vertical)
            .inverted(true)
            .draw_value(true)
            .value_pos(gtk::PositionType::Top)
            .build();
        volume_scale.update_property(&[gtk::accessible::Property::Label(&labels.volume)]);
        volume_scale.set_range(0.0, f64::from(resolved.volume_max));
        volume_scale.set_value(f64::from(resolved.volume_default));
        volume_scale.set_size_request(-1, 150);
        volume_box.append(&volume_scale);
        volume_popover.set_child(Some(&volume_box));
        let volume_button = tooltip::icon_button(
            "audio-volume-high-symbolic",
            &labels.volume,
            &["flat", "circular"],
        );
        attach_popover_to_button(&volume_button, &volume_popover);

        let audio_track_popover = gtk::Popover::new();
        let audio_track_box = popover_box();
        audio_track_box.append(&gtk::Label::new(Some(&labels.no_tracks_loaded)));
        audio_track_popover.set_child(Some(&audio_track_box));
        let audio_track_button = tooltip::icon_button(
            "audio-input-microphone-symbolic",
            &labels.audio_track,
            &["flat", "circular"],
        );
        attach_popover_to_button(&audio_track_button, &audio_track_popover);

        let subtitle_track_popover = gtk::Popover::new();
        let subtitle_track_box = popover_box();
        subtitle_track_box.append(&gtk::Label::new(Some(&labels.no_subtitles)));
        subtitle_track_popover.set_child(Some(&subtitle_track_box));
        let subtitle_track_button = tooltip::icon_button(
            "format-text-underline-symbolic",
            &labels.subtitle_track,
            &["flat", "circular"],
        );
        attach_popover_to_button(&subtitle_track_button, &subtitle_track_popover);

        Self {
            speed_button,
            speed_box,
            volume_button,
            volume_scale,
            audio_track_button,
            subtitle_track_button,
            audio_track_box,
            subtitle_track_box,
        }
    }

    /// Borrow the playback-speed popover button.
    #[must_use]
    pub fn speed_button(&self) -> &gtk::Button {
        &self.speed_button
    }

    /// Borrow the playback-speed popover content box (for runtime label updates).
    #[must_use]
    pub fn speed_box(&self) -> &gtk::Box {
        &self.speed_box
    }

    /// Borrow the volume popover button.
    #[must_use]
    pub fn volume_button(&self) -> &gtk::Button {
        &self.volume_button
    }

    /// Borrow the inner volume `Scale`.
    #[must_use]
    pub fn volume_scale(&self) -> &gtk::Scale {
        &self.volume_scale
    }

    /// Borrow the audio-track popover button.
    #[must_use]
    pub fn audio_track_button(&self) -> &gtk::Button {
        &self.audio_track_button
    }

    /// Borrow the subtitle-track popover button.
    #[must_use]
    pub fn subtitle_track_button(&self) -> &gtk::Button {
        &self.subtitle_track_button
    }

    /// Borrow the audio-track popover content box.
    #[must_use]
    pub fn audio_track_box(&self) -> &gtk::Box {
        &self.audio_track_box
    }

    /// Borrow the subtitle-track popover content box.
    #[must_use]
    pub fn subtitle_track_box(&self) -> &gtk::Box {
        &self.subtitle_track_box
    }
}

fn popover_box() -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 4);
    box_.set_margin_start(8);
    box_.set_margin_end(8);
    box_.set_margin_top(8);
    box_.set_margin_bottom(8);
    box_
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_clamps_defaults() {
        let spec = BigVideoAuxControlSpec::new(BigVideoAuxControlLabels::english(), &["1x"])
            .default_speed_index(99)
            .volume_range(200, 150);
        let resolved = spec.resolved();
        assert_eq!(resolved.speed_count, 1);
        assert_eq!(resolved.default_speed_index, 0);
        assert_eq!(resolved.volume_default, 150);
        assert_eq!(resolved.volume_max, 150);
    }
}
