// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Numeric range shared by the spin inputs.

/// Bounds and steps of a numeric spin input.
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
