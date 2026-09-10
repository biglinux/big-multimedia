// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared audio setting slider rows.

use adw::prelude::*;
use big_relm4_components::layout::illustration_card::slider_spin_row;
use relm4::gtk;

const SLIDER_LOWER_BOUND: f64 = 0.0;
const PERCENT_SLIDER_UPPER_BOUND: f64 = 100.0;
const PERCENT_SLIDER_STEP_INCREMENT: f64 = 1.0;
const PERCENT_SLIDER_PAGE_INCREMENT: f64 = 5.0;
const MARKED_PERCENT_SLIDER_MINIMUM_PAGE_INCREMENT: f64 = 1.0;
const INTEGER_SLIDER_STEP_INCREMENT: f64 = 1.0;
const SMALL_INTEGER_SLIDER_PAGE_INCREMENT: f64 = 5.0;
const LARGE_INTEGER_SLIDER_PAGE_INCREMENT: f64 = 50.0;
const VOLUME_SLIDER_PAGE_INCREMENT: f64 = 10.0;
const SPIN_CLIMB_RATE: f64 = 1.0;
const SPIN_DIGITS: u32 = 0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct SliderAdjustmentSpec {
    initial_value: f64,
    lower_bound: f64,
    upper_bound: f64,
    step_increment: f64,
    page_increment: f64,
    spin_climb_rate: f64,
    spin_digits: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct MarkedPercentSliderSpec {
    adjustment: SliderAdjustmentSpec,
    marks: Vec<SliderMarkSpec>,
}

#[derive(Debug, Clone, PartialEq)]
struct SliderMarkSpec {
    value: f64,
    label: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct VolumeSliderSpec {
    adjustment: SliderAdjustmentSpec,
    full_volume_mark_percent: f64,
}

/// Build a `[label | scale | spin]` row for `0.0..=1.0` settings.
pub fn percent_slider_row<F>(label: &str, initial: f32, on_change: F) -> gtk::Box
where
    F: Fn(f32) + 'static,
{
    let spec = percent_slider_spec(initial);
    let adjustment = gtk::Adjustment::new(
        spec.initial_value,
        spec.lower_bound,
        spec.upper_bound,
        spec.step_increment,
        spec.page_increment,
        0.0,
    );
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    let spin = gtk::SpinButton::new(Some(&adjustment), spec.spin_climb_rate, spec.spin_digits);

    adjustment
        .connect_value_changed(move |adjustment| on_change(percent_to_unit(adjustment.value())));
    slider_spin_row(label, &scale, &spin)
}

/// Build a percent row with labelled marks, still returning `0.0..=1.0`.
pub fn marked_percent_slider_row<F>(
    label: &str,
    initial: f32,
    marks: &[(f64, &str)],
    page_increment: f64,
    on_change: F,
) -> gtk::Box
where
    F: Fn(f32) + 'static,
{
    let spec = marked_percent_slider_spec(initial, marks, page_increment);
    let adjustment = gtk::Adjustment::new(
        spec.adjustment.initial_value,
        spec.adjustment.lower_bound,
        spec.adjustment.upper_bound,
        spec.adjustment.step_increment,
        spec.adjustment.page_increment,
        0.0,
    );
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    for mark in &spec.marks {
        scale.add_mark(
            mark.value,
            gtk::PositionType::Bottom,
            Some(mark.label.as_str()),
        );
    }
    let spin = gtk::SpinButton::new(
        Some(&adjustment),
        spec.adjustment.spin_climb_rate,
        spec.adjustment.spin_digits,
    );

    adjustment
        .connect_value_changed(move |adjustment| on_change(percent_to_unit(adjustment.value())));
    slider_spin_row(label, &scale, &spin)
}

/// Build a `[label | scale | spin]` row for bounded `u8` settings.
pub fn u8_slider_row<F>(label: &str, initial: u8, max: u8, on_change: F) -> gtk::Box
where
    F: Fn(u8) + 'static,
{
    let spec = small_integer_slider_spec(initial, max);
    let adjustment = gtk::Adjustment::new(
        spec.initial_value,
        spec.lower_bound,
        spec.upper_bound,
        spec.step_increment,
        spec.page_increment,
        0.0,
    );
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    let spin = gtk::SpinButton::new(Some(&adjustment), spec.spin_climb_rate, spec.spin_digits);

    adjustment.connect_value_changed(move |adjustment| {
        on_change(clamp_rounded_u8(adjustment.value(), max))
    });
    slider_spin_row(label, &scale, &spin)
}

/// Build a `[label | scale | spin]` row for bounded `u32` settings.
pub fn u32_slider_row<F>(label: &str, initial: u32, max: u32, on_change: F) -> gtk::Box
where
    F: Fn(u32) + 'static,
{
    let spec = large_integer_slider_spec(initial, max);
    let adjustment = gtk::Adjustment::new(
        spec.initial_value,
        spec.lower_bound,
        spec.upper_bound,
        spec.step_increment,
        spec.page_increment,
        0.0,
    );
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    let spin = gtk::SpinButton::new(Some(&adjustment), spec.spin_climb_rate, spec.spin_digits);

    adjustment.connect_value_changed(move |adjustment| {
        on_change(clamp_rounded_u32(adjustment.value(), max))
    });
    slider_spin_row(label, &scale, &spin)
}

/// Build a volume row returning both the row and its adjustment handle.
pub fn volume_slider_row<F>(
    label: &str,
    initial: f32,
    max_percent: f64,
    on_change: F,
) -> (gtk::Box, gtk::Adjustment)
where
    F: Fn(f32) + 'static,
{
    let spec = volume_slider_spec(initial, max_percent);
    let adjustment = gtk::Adjustment::new(
        spec.adjustment.initial_value,
        spec.adjustment.lower_bound,
        spec.adjustment.upper_bound,
        spec.adjustment.step_increment,
        spec.adjustment.page_increment,
        0.0,
    );
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    scale.add_mark(
        spec.full_volume_mark_percent,
        gtk::PositionType::Bottom,
        None,
    );
    let spin = gtk::SpinButton::new(
        Some(&adjustment),
        spec.adjustment.spin_climb_rate,
        spec.adjustment.spin_digits,
    );

    let maximum_percent = spec.adjustment.upper_bound;
    adjustment.connect_value_changed(move |adjustment| {
        on_change(limited_percent_to_unit(adjustment.value(), maximum_percent));
    });
    (slider_spin_row(label, &scale, &spin), adjustment)
}

fn percent_slider_spec(initial: f32) -> SliderAdjustmentSpec {
    SliderAdjustmentSpec {
        initial_value: unit_to_percent(initial),
        lower_bound: SLIDER_LOWER_BOUND,
        upper_bound: PERCENT_SLIDER_UPPER_BOUND,
        step_increment: PERCENT_SLIDER_STEP_INCREMENT,
        page_increment: PERCENT_SLIDER_PAGE_INCREMENT,
        spin_climb_rate: SPIN_CLIMB_RATE,
        spin_digits: SPIN_DIGITS,
    }
}

fn marked_percent_slider_spec(
    initial: f32,
    marks: &[(f64, &str)],
    page_increment: f64,
) -> MarkedPercentSliderSpec {
    MarkedPercentSliderSpec {
        adjustment: SliderAdjustmentSpec {
            page_increment: page_increment.max(MARKED_PERCENT_SLIDER_MINIMUM_PAGE_INCREMENT),
            ..percent_slider_spec(initial)
        },
        marks: marks
            .iter()
            .map(|(value, label)| SliderMarkSpec {
                value: value.clamp(SLIDER_LOWER_BOUND, PERCENT_SLIDER_UPPER_BOUND),
                label: (*label).to_string(),
            })
            .collect(),
    }
}

fn small_integer_slider_spec(initial: u8, max: u8) -> SliderAdjustmentSpec {
    SliderAdjustmentSpec {
        initial_value: f64::from(initial),
        lower_bound: SLIDER_LOWER_BOUND,
        upper_bound: f64::from(max),
        step_increment: INTEGER_SLIDER_STEP_INCREMENT,
        page_increment: SMALL_INTEGER_SLIDER_PAGE_INCREMENT,
        spin_climb_rate: SPIN_CLIMB_RATE,
        spin_digits: SPIN_DIGITS,
    }
}

fn large_integer_slider_spec(initial: u32, max: u32) -> SliderAdjustmentSpec {
    SliderAdjustmentSpec {
        initial_value: f64::from(initial),
        lower_bound: SLIDER_LOWER_BOUND,
        upper_bound: f64::from(max),
        step_increment: INTEGER_SLIDER_STEP_INCREMENT,
        page_increment: LARGE_INTEGER_SLIDER_PAGE_INCREMENT,
        spin_climb_rate: SPIN_CLIMB_RATE,
        spin_digits: SPIN_DIGITS,
    }
}

fn volume_slider_spec(initial: f32, max_percent: f64) -> VolumeSliderSpec {
    let maximum_percent = max_percent.max(1.0);
    VolumeSliderSpec {
        adjustment: SliderAdjustmentSpec {
            initial_value: unit_to_limited_percent(initial, maximum_percent),
            lower_bound: SLIDER_LOWER_BOUND,
            upper_bound: maximum_percent,
            step_increment: PERCENT_SLIDER_STEP_INCREMENT,
            page_increment: VOLUME_SLIDER_PAGE_INCREMENT,
            spin_climb_rate: SPIN_CLIMB_RATE,
            spin_digits: SPIN_DIGITS,
        },
        full_volume_mark_percent: PERCENT_SLIDER_UPPER_BOUND.min(maximum_percent),
    }
}

/// Map a `0.0..=1.0` unit value to a `0..=100` percent display value.
#[must_use]
pub fn unit_to_percent(value: f32) -> f64 {
    (f64::from(value) * 100.0).clamp(0.0, 100.0)
}

/// Inverse of [`unit_to_percent`], clamped to `0.0..=1.0`.
#[must_use]
pub fn percent_to_unit(value: f64) -> f32 {
    (value / 100.0).clamp(0.0, 1.0) as f32
}

/// Map a unit value to a percent clamped against a configurable maximum.
#[must_use]
pub fn unit_to_limited_percent(value: f32, max_percent: f64) -> f64 {
    (f64::from(value) * 100.0).clamp(0.0, max_percent.max(1.0))
}

/// Inverse of [`unit_to_limited_percent`].
#[must_use]
pub fn limited_percent_to_unit(value: f64, max_percent: f64) -> f32 {
    (value / 100.0).clamp(0.0, max_percent.max(1.0) / 100.0) as f32
}

/// Round and clamp `value` into the inclusive range `0..=max` as a `u8`.
#[must_use]
pub fn clamp_rounded_u8(value: f64, max: u8) -> u8 {
    value.round().clamp(0.0, f64::from(max)) as u8
}

/// Round and clamp `value` into the inclusive range `0..=max` as a `u32`.
#[must_use]
pub fn clamp_rounded_u32(value: f64, max: u32) -> u32 {
    value.round().clamp(0.0, f64::from(max)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_slider_spec_uses_unit_percent_range() {
        let spec = percent_slider_spec(0.25);

        assert_eq!(spec.initial_value, 25.0);
        assert_eq!(spec.lower_bound, 0.0);
        assert_eq!(spec.upper_bound, 100.0);
        assert_eq!(spec.step_increment, 1.0);
        assert_eq!(spec.page_increment, 5.0);
        assert_eq!(spec.spin_climb_rate, 1.0);
        assert_eq!(spec.spin_digits, 0);

        assert_eq!(percent_slider_spec(-1.0).initial_value, 0.0);
        assert_eq!(percent_slider_spec(2.0).initial_value, 100.0);
    }

    #[test]
    fn marked_percent_slider_spec_clamps_marks_and_page_increment() {
        let spec = marked_percent_slider_spec(
            0.5,
            &[(-10.0, "Low"), (50.0, "Middle"), (150.0, "High")],
            -2.0,
        );

        assert_eq!(spec.adjustment.initial_value, 50.0);
        assert_eq!(spec.adjustment.page_increment, 1.0);
        assert_eq!(
            spec.marks,
            vec![
                SliderMarkSpec {
                    value: 0.0,
                    label: "Low".into(),
                },
                SliderMarkSpec {
                    value: 50.0,
                    label: "Middle".into(),
                },
                SliderMarkSpec {
                    value: 100.0,
                    label: "High".into(),
                },
            ]
        );
    }

    #[test]
    fn integer_slider_specs_use_expected_ranges() {
        let small = small_integer_slider_spec(7, 10);
        assert_eq!(small.initial_value, 7.0);
        assert_eq!(small.lower_bound, 0.0);
        assert_eq!(small.upper_bound, 10.0);
        assert_eq!(small.step_increment, 1.0);
        assert_eq!(small.page_increment, 5.0);

        let large = large_integer_slider_spec(250, 1_000);
        assert_eq!(large.initial_value, 250.0);
        assert_eq!(large.lower_bound, 0.0);
        assert_eq!(large.upper_bound, 1_000.0);
        assert_eq!(large.step_increment, 1.0);
        assert_eq!(large.page_increment, 50.0);
    }

    #[test]
    fn volume_slider_spec_clamps_maximum_and_full_volume_mark() {
        let boosted = volume_slider_spec(1.25, 150.0);

        assert_eq!(boosted.adjustment.initial_value, 125.0);
        assert_eq!(boosted.adjustment.lower_bound, 0.0);
        assert_eq!(boosted.adjustment.upper_bound, 150.0);
        assert_eq!(boosted.adjustment.step_increment, 1.0);
        assert_eq!(boosted.adjustment.page_increment, 10.0);
        assert_eq!(boosted.full_volume_mark_percent, 100.0);

        let minimal = volume_slider_spec(2.0, -10.0);
        assert_eq!(minimal.adjustment.initial_value, 1.0);
        assert_eq!(minimal.adjustment.upper_bound, 1.0);
        assert_eq!(minimal.full_volume_mark_percent, 1.0);
    }

    #[test]
    fn percent_helpers_clamp_to_unit_range() {
        assert_eq!(unit_to_percent(-1.0), 0.0);
        assert_eq!(unit_to_percent(0.25), 25.0);
        assert_eq!(unit_to_percent(2.0), 100.0);
        assert_eq!(percent_to_unit(-5.0), 0.0);
        assert_eq!(percent_to_unit(25.0), 0.25);
        assert_eq!(percent_to_unit(105.0), 1.0);
    }

    #[test]
    fn integer_helpers_round_and_clamp() {
        assert_eq!(clamp_rounded_u8(-1.0, 10), 0);
        assert_eq!(clamp_rounded_u8(4.6, 10), 5);
        assert_eq!(clamp_rounded_u8(99.0, 10), 10);
        assert_eq!(clamp_rounded_u32(-1.0, 20), 0);
        assert_eq!(clamp_rounded_u32(7.5, 20), 8);
        assert_eq!(clamp_rounded_u32(99.0, 20), 20);
    }

    #[test]
    fn limited_percent_helpers_allow_boosted_volume() {
        assert_eq!(unit_to_limited_percent(2.0, 150.0), 150.0);
        assert_eq!(unit_to_limited_percent(1.25, 150.0), 125.0);
        assert_eq!(limited_percent_to_unit(75.0, 150.0), 0.75);
        assert_eq!(limited_percent_to_unit(150.0, 150.0), 1.5);
        assert_eq!(limited_percent_to_unit(250.0, 150.0), 1.5);
    }
}
