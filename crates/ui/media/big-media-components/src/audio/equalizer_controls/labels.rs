// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Equalizer label helpers.

use big_audio_effects::equalizer::VOICE_EQ_PRESET_IDS;

use crate::text::text;

/// Voice EQ preset display labels.
#[must_use]
pub fn voice_eq_preset_labels() -> Vec<String> {
    VOICE_EQ_PRESET_IDS
        .iter()
        .map(|id| voice_eq_preset_label(id))
        .collect()
}

/// Translate a voice EQ preset id.
#[must_use]
pub fn voice_eq_preset_label(id: &str) -> String {
    match id {
        "default_voice" => text("Default voice"),
        "flat" => text("Flat"),
        "voice_boost" => text("Voice boost"),
        "podcast" => text("Podcast"),
        "warm" => text("Warm"),
        "bright" => text("Bright"),
        "de_esser" => text("De-esser"),
        "bass_cut" => text("Bass cut"),
        "presence" => text("Presence"),
        "custom" => text("Custom"),
        other => other.to_owned(),
    }
}

/// Compact frequency label for the vertical equalizer, e.g. `"31"` or `"16k"`.
#[must_use]
pub fn compact_frequency_label(frequency_hz: u32) -> String {
    if frequency_hz >= 1000 {
        format!("{}k", frequency_hz / 1000)
    } else {
        format!("{frequency_hz}")
    }
}

/// Frequency label for the horizontal equalizer rows, e.g. `"80 Hz"` or `"1 kHz"`.
#[must_use]
pub fn frequency_label(frequency_hz: u32) -> String {
    if frequency_hz >= 1000 {
        format!("{} kHz", frequency_hz / 1000)
    } else {
        format!("{frequency_hz} Hz")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_frequency_label_uses_short_k_suffix() {
        assert_eq!(compact_frequency_label(31), "31");
        assert_eq!(compact_frequency_label(1_000), "1k");
        assert_eq!(compact_frequency_label(16_000), "16k");
    }

    #[test]
    fn frequency_label_uses_units() {
        assert_eq!(frequency_label(80), "80 Hz");
        assert_eq!(frequency_label(1_000), "1 kHz");
    }
}
