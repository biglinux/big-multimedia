// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Equalizer settings parsing helpers.

/// Parse a `;`-separated list of band gains into a fixed-size array, clamped to `[min, max]`.
///
/// Missing or unparsable entries default to `0.0`. Extra entries beyond
/// `N` are ignored. Used to restore persisted equalizer state from a
/// settings string.
#[must_use]
pub fn parse_clamped_band_gains<const N: usize>(
    gains_text: &str,
    min_gain: f64,
    max_gain: f64,
) -> [f64; N] {
    let mut gains = [0.0_f64; N];
    for (index, gain) in gains_text
        .split(';')
        .filter_map(|item| item.parse::<f64>().ok())
        .enumerate()
    {
        if index >= N {
            break;
        }
        gains[index] = gain.clamp(min_gain, max_gain);
    }
    gains
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clamped_band_gains_pads_and_clamps() {
        let gains = parse_clamped_band_gains::<4>("100;-100;3;bad;5", -12.0, 12.0);
        assert_eq!(gains, [12.0, -12.0, 3.0, 5.0]);
    }

    #[test]
    fn parse_clamped_band_gains_ignores_extra_values() {
        let gains = parse_clamped_band_gains::<2>("1;2;3;4", -12.0, 12.0);
        assert_eq!(gains, [1.0, 2.0]);
    }
}
