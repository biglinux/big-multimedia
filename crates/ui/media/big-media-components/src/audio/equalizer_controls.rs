// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Reusable equalizer preset + band controls.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use big_audio_effects::equalizer::{
    EQ_BAND_FREQUENCIES_HZ, VOICE_EQ_PRESET_BANDS, VOICE_EQ_PRESET_IDS,
};
use big_relm4_components::input::labeled_scale_row::{BigLabeledScaleRow, BigLabeledScaleRowSpec};
use big_relm4_components::layout::illustration_card::labelled_row;
use relm4::gtk;

use crate::text::text;

const DEFAULT_PRESET_SPACING: i32 = 12;
const DEFAULT_VERTICAL_BAND_HEIGHT: i32 = 140;
const DEFAULT_VERTICAL_BAND_WIDTH: i32 = 48;

mod labels;
mod parsing;
mod vertical;

pub use labels::{
    compact_frequency_label, frequency_label, voice_eq_preset_label, voice_eq_preset_labels,
};
pub use parsing::parse_clamped_band_gains;

/// Translated labels rendered next to the equalizer preset dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAudioEqualizerLabels {
    /// Label preceding the preset dropdown, e.g. `"Preset"`.
    pub preset: String,
}

impl BigAudioEqualizerLabels {
    /// Hard-coded English defaults useful for unit tests.
    #[must_use]
    pub fn english() -> Self {
        Self {
            preset: "Preset".into(),
        }
    }
}

/// Builder spec for [`BigAudioEqualizerControls`].
///
/// Holds preset and frequency metadata, plus optional layout overrides used
/// to fit the equalizer rows into a dialog, a sidebar, or a settings page.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq)]
pub struct BigAudioEqualizerSpec {
    pub labels: BigAudioEqualizerLabels,
    pub frequencies_hz: Vec<u32>,
    pub preset_labels: Vec<String>,
    pub preset_bands: Vec<Vec<f64>>,
    pub preset_row_spacing: i32,
    pub preset_margin_bottom: i32,
    pub band_title_width_chars: Option<i32>,
    pub band_value_width_chars: Option<i32>,
    pub band_spacing: Option<i32>,
    pub band_height_request: Option<i32>,
    pub band_margin_y: i32,
}

impl BigAudioEqualizerSpec {
    /// Build a spec from translated labels, frequency list, preset names, and preset band tables.
    #[must_use]
    pub fn new(
        labels: BigAudioEqualizerLabels,
        frequencies_hz: impl IntoIterator<Item = u32>,
        preset_labels: impl IntoIterator<Item = impl Into<String>>,
        preset_bands: impl IntoIterator<Item = impl IntoIterator<Item = f64>>,
    ) -> Self {
        Self {
            labels,
            frequencies_hz: frequencies_hz.into_iter().collect(),
            preset_labels: preset_labels.into_iter().map(Into::into).collect(),
            preset_bands: preset_bands
                .into_iter()
                .map(|band| band.into_iter().collect())
                .collect(),
            preset_row_spacing: DEFAULT_PRESET_SPACING,
            preset_margin_bottom: 0,
            band_title_width_chars: None,
            band_value_width_chars: None,
            band_spacing: None,
            band_height_request: None,
            band_margin_y: 0,
        }
    }

    /// Override preset-row spacing and bottom margin.
    #[must_use]
    pub fn preset_layout(mut self, spacing: i32, margin_bottom: i32) -> Self {
        self.preset_row_spacing = spacing.max(0);
        self.preset_margin_bottom = margin_bottom.max(0);
        self
    }

    /// Override band-row widget sizing (title/value width, spacing, height, vertical margin).
    #[must_use]
    pub fn band_layout(
        mut self,
        title_width_chars: i32,
        value_width_chars: i32,
        spacing: i32,
        height_request: i32,
        margin_y: i32,
    ) -> Self {
        self.band_title_width_chars = Some(title_width_chars.max(1));
        self.band_value_width_chars = Some(value_width_chars.max(1));
        self.band_spacing = Some(spacing.max(0));
        self.band_height_request = Some(height_request.max(1));
        self.band_margin_y = margin_y.max(0);
        self
    }

    /// Return a resolved view summarising counts and structural consistency.
    #[must_use]
    pub fn resolved(&self) -> BigAudioEqualizerResolved {
        BigAudioEqualizerResolved {
            band_count: self.frequencies_hz.len(),
            preset_count: self.preset_labels.len(),
            preset_rows_match_bands: self
                .preset_bands
                .iter()
                .all(|preset| preset.len() == self.frequencies_hz.len()),
        }
    }
}

/// Resolved view of a [`BigAudioEqualizerSpec`] for tests and validators.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigAudioEqualizerResolved {
    pub band_count: usize,
    pub preset_count: usize,
    pub preset_rows_match_bands: bool,
}

/// Preset dropdown + horizontal band scales bound to caller-owned adjustments.
#[derive(Debug, Clone)]
pub struct BigAudioEqualizerControls {
    root: gtk::Box,
    preset_dropdown: gtk::DropDown,
    band_scales: Rc<RefCell<Vec<gtk::Scale>>>,
}

/// Labels used by the vertical equalizer controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAudioEqualizerVerticalLabels {
    /// Label preceding the preset dropdown.
    pub preset: String,
}

impl BigAudioEqualizerVerticalLabels {
    /// Build labels using the crate-wide [`crate::text::text`] translator.
    #[must_use]
    pub fn translated() -> Self {
        Self {
            preset: text("Preset"),
        }
    }
}

/// Voice-focused vertical equalizer control spec.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq)]
pub struct BigAudioEqualizerVerticalSpec {
    pub labels: BigAudioEqualizerVerticalLabels,
    pub frequencies_hz: Vec<u32>,
    pub preset_ids: Vec<&'static str>,
    pub preset_labels: Vec<String>,
    pub preset_bands: Vec<Vec<f32>>,
    pub initial_preset: String,
    pub initial_bands: Vec<f32>,
    pub min_gain_db: f32,
    pub max_gain_db: f32,
    pub band_height_request: i32,
    pub band_width_request: i32,
}

impl BigAudioEqualizerVerticalSpec {
    /// Build a spec from translated labels, frequencies, preset ids, labels, band tables, and initial state.
    #[must_use]
    pub fn new(
        labels: BigAudioEqualizerVerticalLabels,
        frequencies_hz: impl IntoIterator<Item = u32>,
        preset_ids: impl IntoIterator<Item = &'static str>,
        preset_labels: impl IntoIterator<Item = impl Into<String>>,
        preset_bands: impl IntoIterator<Item = impl IntoIterator<Item = f32>>,
        initial_preset: impl Into<String>,
        initial_bands: impl IntoIterator<Item = f32>,
    ) -> Self {
        Self {
            labels,
            frequencies_hz: frequencies_hz.into_iter().collect(),
            preset_ids: preset_ids.into_iter().collect(),
            preset_labels: preset_labels.into_iter().map(Into::into).collect(),
            preset_bands: preset_bands
                .into_iter()
                .map(|band| band.into_iter().collect())
                .collect(),
            initial_preset: initial_preset.into(),
            initial_bands: initial_bands.into_iter().collect(),
            min_gain_db: -40.0,
            max_gain_db: 40.0,
            band_height_request: DEFAULT_VERTICAL_BAND_HEIGHT,
            band_width_request: DEFAULT_VERTICAL_BAND_WIDTH,
        }
    }

    /// Build a spec pre-populated with the voice-focused presets from `big_audio_effects`.
    #[must_use]
    pub fn voice(
        initial_preset: impl Into<String>,
        initial_bands: impl IntoIterator<Item = f32>,
    ) -> Self {
        Self::new(
            BigAudioEqualizerVerticalLabels::translated(),
            EQ_BAND_FREQUENCIES_HZ,
            VOICE_EQ_PRESET_IDS,
            voice_eq_preset_labels(),
            VOICE_EQ_PRESET_BANDS,
            initial_preset,
            initial_bands,
        )
    }

    /// Override the displayed gain range; values are normalized so `min <= max`.
    #[must_use]
    pub fn gain_range(mut self, min_gain_db: f32, max_gain_db: f32) -> Self {
        self.min_gain_db = min_gain_db.min(max_gain_db);
        self.max_gain_db = max_gain_db.max(min_gain_db);
        self
    }

    /// Override per-band size request. Each dimension is clamped to at least 1.
    #[must_use]
    pub fn band_size(mut self, width: i32, height: i32) -> Self {
        self.band_width_request = width.max(1);
        self.band_height_request = height.max(1);
        self
    }

    /// Return a resolved view summarising counts and structural consistency.
    #[must_use]
    pub fn resolved(&self) -> BigAudioEqualizerVerticalResolved {
        BigAudioEqualizerVerticalResolved {
            band_count: self.frequencies_hz.len(),
            preset_count: self.preset_ids.len(),
            preset_labels_match_ids: self.preset_labels.len() == self.preset_ids.len(),
            preset_rows_match_bands: self
                .preset_bands
                .iter()
                .all(|preset| preset.len() == self.frequencies_hz.len()),
            initial_bands_match: self.initial_bands.len() == self.frequencies_hz.len(),
        }
    }
}

/// Resolved view of a [`BigAudioEqualizerVerticalSpec`] for tests and validators.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigAudioEqualizerVerticalResolved {
    pub band_count: usize,
    pub preset_count: usize,
    pub preset_labels_match_ids: bool,
    pub preset_rows_match_bands: bool,
    pub initial_bands_match: bool,
}

/// Vertical preset dropdown + 10-band scale row.
#[derive(Debug, Clone)]
pub struct BigAudioEqualizerVerticalControls {
    root: gtk::Box,
    preset_dropdown: gtk::DropDown,
    band_scales: Rc<Vec<gtk::Scale>>,
}

impl BigAudioEqualizerVerticalControls {
    /// Build the vertical equalizer widget tree.
    #[must_use]
    pub fn new(spec: BigAudioEqualizerVerticalSpec) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let preset_dropdown = vertical::preset_dropdown(&spec);
        root.append(&labelled_row(&spec.labels.preset, &preset_dropdown));

        let band_scales = Rc::new(vertical::build_band_scales(&spec));
        vertical::apply_band_values(&band_scales, &spec.initial_bands);
        root.append(&vertical::bands_row(&spec, &band_scales));

        Self {
            root,
            preset_dropdown,
            band_scales,
        }
    }

    /// Borrow the root [`gtk::Box`] for embedding.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Consume `self` and return the root [`gtk::Box`].
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }

    /// Borrow the preset dropdown bound to the band scales.
    #[must_use]
    pub fn preset_dropdown(&self) -> &gtk::DropDown {
        &self.preset_dropdown
    }

    /// Borrow the per-band scale widgets through a shared `Rc<Vec>`.
    #[must_use]
    pub fn band_scales(&self) -> Rc<Vec<gtk::Scale>> {
        self.band_scales.clone()
    }
}

impl BigAudioEqualizerControls {
    /// Build the equalizer widget tree mirroring `source_preset_dropdown` and `band_adjustments`.
    ///
    /// Selecting a preset writes the corresponding gains into both the
    /// internal scales and the caller-owned adjustments.
    #[must_use]
    pub fn new(
        spec: BigAudioEqualizerSpec,
        source_preset_dropdown: &gtk::DropDown,
        band_adjustments: &[gtk::Adjustment],
    ) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);

        let preset_labels = spec
            .preset_labels
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let preset_dropdown = gtk::DropDown::from_strings(&preset_labels);
        sync_dropdowns(&preset_dropdown, source_preset_dropdown);

        let preset_row = gtk::Box::new(gtk::Orientation::Horizontal, spec.preset_row_spacing);
        preset_row.set_margin_bottom(spec.preset_margin_bottom);
        preset_row.append(
            &gtk::Label::builder()
                .label(&spec.labels.preset)
                .hexpand(true)
                .halign(gtk::Align::Start)
                .build(),
        );
        preset_row.append(&preset_dropdown);
        root.append(&preset_row);

        let band_scales = Rc::new(RefCell::new(Vec::new()));
        for (index, frequency) in spec.frequencies_hz.iter().copied().enumerate() {
            let Some(adjustment) = band_adjustments.get(index) else {
                continue;
            };
            let (scale, row) = make_band_row(&spec, &frequency_label(frequency), adjustment);
            root.append(&row);
            band_scales.borrow_mut().push(scale);
        }

        {
            let scales = band_scales.clone();
            let adjustments = band_adjustments.to_vec();
            let preset_bands = spec.preset_bands.clone();
            preset_dropdown.connect_selected_notify(move |dropdown| {
                let Some(preset) = preset_bands.get(dropdown.selected() as usize) else {
                    return;
                };
                for (index, value) in preset.iter().copied().enumerate() {
                    if let Some(scale) = scales.borrow().get(index) {
                        scale.set_value(value);
                    }
                    if let Some(adjustment) = adjustments.get(index) {
                        adjustment.set_value(value);
                    }
                }
            });
        }

        Self {
            root,
            preset_dropdown,
            band_scales,
        }
    }

    /// Borrow the root [`gtk::Box`] for embedding.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Consume `self` and return the root [`gtk::Box`].
    #[must_use]
    pub fn into_root(self) -> gtk::Box {
        self.root
    }

    /// Borrow the preset dropdown bound to the band scales.
    #[must_use]
    pub fn preset_dropdown(&self) -> &gtk::DropDown {
        &self.preset_dropdown
    }

    /// Borrow the per-band scale widgets through a shared `Rc<RefCell>`.
    #[must_use]
    pub fn band_scales(&self) -> Rc<RefCell<Vec<gtk::Scale>>> {
        self.band_scales.clone()
    }
}

fn sync_dropdowns(dialog_dropdown: &gtk::DropDown, source_dropdown: &gtk::DropDown) {
    // Delegate to the shared weak-bind helper — a local two-way strong bind is a
    // mutual reference cycle that leaks both dropdowns when the dialog closes.
    big_relm4_components::input::dropdown::sync_dropdowns(dialog_dropdown, source_dropdown);
}

fn make_band_row(
    spec: &BigAudioEqualizerSpec,
    label: &str,
    adjustment: &gtk::Adjustment,
) -> (gtk::Scale, gtk::Box) {
    let labeled = BigLabeledScaleRow::new(equalizer_band_row_spec(spec, label), adjustment);
    let (scale, row, _) = labeled.into_parts();
    row.set_margin_top(spec.band_margin_y);
    row.set_margin_bottom(spec.band_margin_y);
    (scale, row)
}

fn equalizer_band_row_spec(spec: &BigAudioEqualizerSpec, label: &str) -> BigLabeledScaleRowSpec {
    let mut row_spec = BigLabeledScaleRowSpec::new(label)
        .display(1.0, " dB", 1)
        .mark_zero(true);

    if let Some(width) = spec.band_title_width_chars {
        row_spec = row_spec.title_width_chars(width);
    }
    if let Some(width) = spec.band_value_width_chars {
        row_spec = row_spec.value_width_chars(width);
    }
    if let Some(spacing) = spec.band_spacing {
        row_spec = row_spec.spacing(spacing);
    }
    if let Some(height) = spec.band_height_request {
        row_spec = row_spec.scale_height_request(height);
    }

    row_spec
}

#[cfg(test)]
mod tests {
    use super::{BigAudioEqualizerLabels, BigAudioEqualizerSpec, equalizer_band_row_spec};

    fn single_band_spec() -> BigAudioEqualizerSpec {
        BigAudioEqualizerSpec::new(
            BigAudioEqualizerLabels::english(),
            [60],
            ["Flat"],
            [vec![0.0]],
        )
    }

    #[test]
    fn band_row_spec_uses_decibel_display_and_zero_mark() {
        let spec = single_band_spec();
        let row_spec = equalizer_band_row_spec(&spec, "60 Hz");

        assert_eq!(row_spec.title, "60 Hz");
        assert_eq!(row_spec.display_multiplier, 1.0);
        assert_eq!(row_spec.suffix, " dB");
        assert_eq!(row_spec.digits, 1);
        assert!(row_spec.mark_zero);
        assert_eq!(row_spec.value_text(2.34), "2.3 dB");
    }

    #[test]
    fn band_row_spec_applies_optional_layout_overrides() {
        let spec = single_band_spec().band_layout(9, 10, 11, 12, 2);
        let row_spec = equalizer_band_row_spec(&spec, "1 kHz");

        assert_eq!(row_spec.title_width_chars, 9);
        assert_eq!(row_spec.value_width_chars, 10);
        assert_eq!(row_spec.spacing, 11);
        assert_eq!(row_spec.scale_height_request, Some(12));
    }
}
