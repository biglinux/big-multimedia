// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Runtime configuration for BigLinux custom tooltips.

const DEFAULT_SHOW_DELAY_MS: u32 = 150;
const DEFAULT_FADE_OUT_MS: u32 = 300;
const DEFAULT_CSS_FADE_MS: u32 = 200;
const DEFAULT_MAX_WIDTH_CHARS: i32 = 80;

/// Runtime tooltip behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigTooltipConfig {
    /// Show delay ms.
    pub show_delay_ms: u32,
    /// Fade out ms.
    pub fade_out_ms: u32,
    /// Css fade ms.
    pub css_fade_ms: u32,
    /// Max width chars.
    pub max_width_chars: i32,
}

impl Default for BigTooltipConfig {
    fn default() -> Self {
        Self {
            show_delay_ms: DEFAULT_SHOW_DELAY_MS,
            fade_out_ms: DEFAULT_FADE_OUT_MS,
            css_fade_ms: DEFAULT_CSS_FADE_MS,
            max_width_chars: DEFAULT_MAX_WIDTH_CHARS,
        }
    }
}
