// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Equalizer settings parsing.

/// Parse a `;`-separated list of band gains into a fixed-size array, clamped to `[min, max]`.
///
/// Missing, unparsable or non-finite entries become `0.0` in place. Extra entries beyond
/// `N` are ignored. Restores persisted equalizer state from a settings
/// string.
#[must_use]
pub fn parse_clamped_band_gains<const N: usize>(
    gains_text: &str,
    min_gain: f64,
    max_gain: f64,
) -> [f64; N] {
    let mut gains = [0.0_f64; N];
    for (slot, item) in gains.iter_mut().zip(gains_text.split(';')) {
        if let Ok(gain) = item.parse::<f64>()
            && gain.is_finite()
        {
            *slot = gain.clamp(min_gain, max_gain);
        }
    }
    gains
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clamped_band_gains_pads_and_clamps() {
        let gains = parse_clamped_band_gains::<4>("100;-100;nan;3;bad;inf;5", -12.0, 12.0);
        assert_eq!(gains, [12.0, -12.0, 0.0, 3.0]);
    }

    /// Settings strings are written as `{:.1}` gains joined by `;`, as the equalizer page saves them.
    #[test]
    fn saved_gains_parse_back_in_place() {
        for seed in 0..500_i32 {
            let tenths: Vec<i32> = (0..10_i32)
                .map(|band| (seed * 37 + band * 101).rem_euclid(241) - 120)
                .collect();
            let gains: Vec<f64> = tenths.iter().map(|&t| f64::from(t) / 10.0).collect();
            let entries: Vec<String> = gains.iter().map(|gain| format!("{gain:.1}")).collect();
            assert_eq!(
                parse_clamped_band_gains::<10>(&entries.join(";"), -12.0, 12.0).to_vec(),
                gains
            );
            for corrupt in 0..10 {
                for bad in ["", "bad", "nan", "inf", "1e400"] {
                    let mut corrupted = entries.clone();
                    corrupted[corrupt] = bad.to_owned();
                    let mut expected = gains.clone();
                    expected[corrupt] = 0.0;
                    assert_eq!(
                        parse_clamped_band_gains::<10>(&corrupted.join(";"), -12.0, 12.0).to_vec(),
                        expected,
                        "band {corrupt} = {bad:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn parse_clamped_band_gains_ignores_extra_values() {
        let gains = parse_clamped_band_gains::<2>("1;2;3;4", -12.0, 12.0);
        assert_eq!(gains, [1.0, 2.0]);
    }
}
