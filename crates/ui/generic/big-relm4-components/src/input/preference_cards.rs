// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Didactic preference cards with app-wired state.

use relm4::gtk;
use relm4::gtk::prelude::*;

use crate::layout::didactic_card::{BigDidacticCardSpec, didactic_card};

const DEFAULT_SPIN_WIDTH_CHARS: i32 = 7;

/// Numeric range descriptor reused by spin and scale preference cards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BigSpinRange {
    /// Lowest legal value.
    pub min: f64,
    /// Highest legal value (inclusive).
    pub max: f64,
    /// Increment when pressing the spin arrows or arrow keys.
    pub step: f64,
    /// Increment when pressing PageUp/PageDown.
    pub page_step: f64,
}

impl BigSpinRange {
    /// Construct a spin range with explicit bounds and step increments.
    #[must_use]
    pub const fn new(min: f64, max: f64, step: f64, page_step: f64) -> Self {
        Self {
            min,
            max,
            step,
            page_step,
        }
    }
}

/// Didactic card carrying a boolean toggle on the right.
#[derive(Debug, Clone)]
pub struct BigSwitchPreferenceCard {
    root: gtk::Box,
    switch: gtk::Switch,
}

impl BigSwitchPreferenceCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDidacticCardSpec, active: bool) -> Self {
        let switch = gtk::Switch::builder()
            .active(active)
            .valign(gtk::Align::Center)
            .build();
        switch.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        let root = didactic_card(spec, &switch);
        Self { root, switch }
    }

    /// Return a reference to the `root` exposed by this [`BigSwitchPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `switch` exposed by this [`BigSwitchPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn switch(&self) -> &gtk::Switch {
        &self.switch
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, gtk::Switch) {
        (self.root, self.switch)
    }
}

/// Didactic card carrying a numeric spin button on the right.
#[derive(Debug, Clone)]
pub struct BigSpinPreferenceCard {
    root: gtk::Box,
    spin: gtk::SpinButton,
}

impl BigSpinPreferenceCard {
    /// Creates a new instance.
    #[must_use]
    pub fn new(spec: BigDidacticCardSpec, value: f64, range: BigSpinRange, digits: u32) -> Self {
        let adjustment = gtk::Adjustment::new(
            value,
            range.min,
            range.max,
            range.step,
            range.page_step,
            0.0,
        );
        let spin = gtk::SpinButton::builder()
            .adjustment(&adjustment)
            .digits(digits)
            .numeric(true)
            .valign(gtk::Align::Center)
            .width_chars(DEFAULT_SPIN_WIDTH_CHARS)
            .build();
        spin.update_property(&[gtk::accessible::Property::Label(&spec.title)]);
        let root = didactic_card(spec, &spin);
        Self { root, spin }
    }

    /// Return a reference to the `root` exposed by this [`BigSpinPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Return a reference to the `spin` exposed by this [`BigSpinPreferenceCard`].
    ///
    /// Useful when callers need to bind signals, query state, or compose the
    /// widget into a larger surface without taking ownership.
    #[must_use]
    pub fn spin(&self) -> &gtk::SpinButton {
        &self.spin
    }

    /// Consume `self` and yield the underlying parts.
    #[must_use]
    pub fn into_parts(self) -> (gtk::Box, gtk::SpinButton) {
        (self.root, self.spin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spin_range_records_bounds() {
        let range = BigSpinRange::new(0.0, 10.0, 1.0, 5.0);
        assert_eq!(range.min, 0.0);
        assert_eq!(range.max, 10.0);
        assert_eq!(range.step, 1.0);
        assert_eq!(range.page_step, 5.0);
    }
}
