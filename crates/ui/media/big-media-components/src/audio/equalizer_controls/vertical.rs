// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Vertical equalizer widget builders.

use relm4::gtk;
use relm4::gtk::prelude::*;

use super::{BigAudioEqualizerVerticalSpec, compact_frequency_label};

pub(super) fn preset_dropdown(spec: &BigAudioEqualizerVerticalSpec) -> gtk::DropDown {
    let label_refs: Vec<&str> = spec.preset_labels.iter().map(String::as_str).collect();
    let dropdown = gtk::DropDown::from_strings(&label_refs);
    if let Some(index) = spec
        .preset_ids
        .iter()
        .position(|id| *id == spec.initial_preset)
    {
        dropdown.set_selected(index as u32);
    }
    dropdown.set_hexpand(true);
    dropdown
}

pub(super) fn build_band_scales(spec: &BigAudioEqualizerVerticalSpec) -> Vec<gtk::Scale> {
    spec.frequencies_hz
        .iter()
        .map(|_| {
            let adj = gtk::Adjustment::new(
                0.0,
                f64::from(spec.min_gain_db),
                f64::from(spec.max_gain_db),
                1.0,
                5.0,
                0.0,
            );
            let scale = gtk::Scale::new(gtk::Orientation::Vertical, Some(&adj));
            scale.set_inverted(true);
            scale.set_draw_value(true);
            scale.set_value_pos(gtk::PositionType::Bottom);
            scale.set_digits(0);
            scale.set_height_request(spec.band_height_request);
            scale.set_width_request(spec.band_width_request);
            scale.set_valign(gtk::Align::Fill);
            scale.add_mark(0.0, gtk::PositionType::Right, None);
            scale
        })
        .collect()
}

pub(super) fn apply_band_values(scales: &[gtk::Scale], bands: &[f32]) {
    for (scale, gain) in scales.iter().zip(bands.iter()) {
        scale.adjustment().set_value(f64::from(*gain));
    }
}

pub(super) fn bands_row(spec: &BigAudioEqualizerVerticalSpec, scales: &[gtk::Scale]) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(16)
        .halign(gtk::Align::Center)
        .build();

    for (scale, freq) in scales.iter().zip(spec.frequencies_hz.iter()) {
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .build();
        column.append(scale);
        let freq_label = gtk::Label::builder()
            .label(compact_frequency_label(*freq))
            .css_classes(vec!["caption", "dim-label"])
            .build();
        column.append(&freq_label);
        row.append(&column);
    }
    row
}
