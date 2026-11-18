// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! BigLinux action row with an accessible spin-button suffix, and an
//! `AdwSpinRow` that screen readers can reach.

use adw::prelude::*;
use relm4::gtk;

use super::preference_cards::{BigSpinRange, finite_or};

const DEFAULT_WIDTH_CHARS: i32 = 7;

/// Data needed to build a spin row.
#[derive(Debug, Clone, PartialEq)]
pub struct BigSpinRowSpec {
    /// Title.
    pub title: String,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Current value.
    pub value: f64,
    /// Numeric range.
    pub range: BigSpinRange,
    /// Decimal digits.
    pub digits: u32,
    /// Spin button width in characters.
    pub width_chars: i32,
}

impl BigSpinRowSpec {
    /// Creates a new instance.
    #[must_use]
    pub fn new(title: impl Into<String>, value: f64, range: BigSpinRange) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            value,
            range,
            digits: 0,
            width_chars: DEFAULT_WIDTH_CHARS,
        }
    }

    /// Configure the `subtitle` setting and return the updated builder.
    ///
    /// The supplied `subtitle` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSpinRowSpec`].
    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Configure the `digits` setting and return the updated builder.
    ///
    /// The supplied `digits` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSpinRowSpec`].
    #[must_use]
    pub fn digits(mut self, digits: u32) -> Self {
        self.digits = digits;
        self
    }

    /// Configure the `width_chars` setting and return the updated builder.
    ///
    /// The supplied `width_chars` overrides any previously stored value; chain
    /// further `with_*` / setter calls to compose the full [`BigSpinRowSpec`].
    #[must_use]
    pub fn width_chars(mut self, width_chars: i32) -> Self {
        self.width_chars = width_chars;
        self
    }

    /// Resolve the spec into its display-free runtime form.
    #[must_use]
    pub fn resolved(&self) -> BigSpinRowResolved {
        let BigSpinRange {
            min,
            max,
            step,
            page_step,
        } = self.range.resolved();

        BigSpinRowResolved {
            value: finite_or(self.value, min).clamp(min, max),
            min,
            max,
            step,
            page_step,
            digits: self.digits,
            width_chars: self.width_chars.max(1),
        }
    }
}

/// Pure spin row contract. Safe for no-display tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BigSpinRowResolved {
    /// Current value.
    pub value: f64,
    /// Lowest legal value.
    pub min: f64,
    /// Highest legal value.
    pub max: f64,
    /// Step increment.
    pub step: f64,
    /// Page increment.
    pub page_step: f64,
    /// Decimal digits.
    pub digits: u32,
    /// Spin button width in characters.
    pub width_chars: i32,
}

/// Built action row plus direct handle for app wiring.
#[derive(Debug, Clone)]
pub struct BigSpinRow {
    row: adw::ActionRow,
    spin: gtk::SpinButton,
}

impl BigSpinRow {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigSpinRowSpec) -> Self {
        let spin = accessible_spin_button(
            &spec.title,
            spec.range,
            spec.value,
            spec.digits,
            spec.width_chars,
        );
        spin.set_valign(gtk::Align::Center);
        if let Some(subtitle) = spec.subtitle.as_deref() {
            spin.update_property(&[gtk::accessible::Property::Description(subtitle)]);
        }

        let mut row_builder = adw::ActionRow::builder().title(&spec.title);
        if let Some(subtitle) = spec.subtitle.as_deref() {
            row_builder = row_builder.subtitle(subtitle);
        }
        let row = row_builder.build();
        row.add_suffix(&spin);
        row.set_activatable_widget(Some(&spin));

        Self { row, spin }
    }

    /// Return a reference to the `row` exposed by this [`BigSpinRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn row(&self) -> &adw::ActionRow {
        &self.row
    }

    /// Return a reference to the `spin` exposed by this [`BigSpinRow`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn spin(&self) -> &gtk::SpinButton {
        &self.spin
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (adw::ActionRow, gtk::SpinButton) {
        (self.row, self.spin)
    }
}

/// An `AdwSpinRow` builder whose spin button reaches screen readers.
///
/// libadwaita 1.10 gives the row the presentation role, and GTK 4.24's AT-SPI
/// bridge drops a presentation widget with everything below it; the generic
/// role keeps the row itself out of the tree while its spin button stays.
pub fn accessible_spin_row() -> adw::builders::SpinRowBuilder {
    #[expect(
        clippy::disallowed_methods,
        reason = "the owner of accessible spin rows"
    )]
    adw::SpinRow::builder().accessible_role(gtk::AccessibleRole::Generic)
}

/// A numeric spin button over `range`, resolved as [`BigSpinRange::resolved`],
/// starting at `value` clamped into it and named `label` for screen readers.
#[must_use]
pub fn accessible_spin_button(
    label: &str,
    range: BigSpinRange,
    value: f64,
    digits: u32,
    width_chars: i32,
) -> gtk::SpinButton {
    let range = range.resolved();
    let adjustment = gtk::Adjustment::new(
        finite_or(value, range.min).clamp(range.min, range.max),
        range.min,
        range.max,
        range.step,
        range.page_step,
        0.0,
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "the owner of accessible spin buttons"
    )]
    let spin = gtk::SpinButton::builder()
        .adjustment(&adjustment)
        .digits(digits)
        .numeric(true)
        .width_chars(width_chars.max(1))
        .build();
    spin.update_property(&[gtk::accessible::Property::Label(label)]);
    spin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_clamps_value_and_width() {
        let resolved = BigSpinRowSpec::new(
            "Maximum devices",
            50.0,
            BigSpinRange::new(1.0, 20.0, 1.0, 10.0),
        )
        .width_chars(0)
        .resolved();

        assert_eq!(resolved.value, 20.0);
        assert_eq!(resolved.width_chars, 1);
    }

    #[test]
    fn resolved_normalizes_invalid_numbers() {
        let resolved = BigSpinRowSpec::new(
            "Frame rate",
            f64::NAN,
            BigSpinRange::new(f64::NAN, f64::NAN, 0.0, f64::NAN),
        )
        .resolved();

        assert_eq!(resolved.value, 0.0);
        assert_eq!(resolved.min, 0.0);
        assert_eq!(resolved.max, 0.0);
        assert!(resolved.step > 0.0);
        assert!(resolved.page_step >= resolved.step);
    }
}
