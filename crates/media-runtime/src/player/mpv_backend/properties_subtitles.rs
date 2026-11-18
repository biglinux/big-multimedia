//! Subtitle properties: appearance, colors, delay, language, SDH, secondary subs,
//! closed captions, image subs, border/scaling.

use super::MpvBackend;
use std::sync::atomic::Ordering;

impl MpvBackend {
    pub fn set_subtitle_track(&self, id: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sid", id);
        });
    }

    pub fn add_subtitle_file(&self, path: &str) {
        let added = self.with_mpv(|m| m.command("sub-add", &[path, "auto"]).is_ok());
        // The subtitle menu lists the new track only when told.
        if added {
            self.republish_tracks();
        }
    }

    /// Largest subtitle delay, earlier or later, in seconds.
    pub const SUB_DELAY_LIMIT_SECONDS: f64 = 10.0;

    /// Seconds the subtitles show after the speech; negative is earlier.
    pub fn sub_delay(&self) -> f64 {
        self.get_property_f64("sub-delay")
    }

    /// Held within ±[`Self::SUB_DELAY_LIMIT_SECONDS`].
    pub fn set_sub_delay(&self, delay: f64) {
        let delay = delay.clamp(
            -Self::SUB_DELAY_LIMIT_SECONDS,
            Self::SUB_DELAY_LIMIT_SECONDS,
        );
        self.with_mpv(|m| {
            let _ = m.set_property("sub-delay", delay);
        });
    }

    pub fn set_sub_font_size(&self, size: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-font-size", size);
        });
    }

    /// Empty is the default font, mpv's own `sans-serif`.
    pub fn set_sub_font(&self, font: &str) {
        let font = if font.is_empty() { "sans-serif" } else { font };
        self.with_mpv(|m| {
            let _ = m.set_property("sub-font", font.to_string());
        });
    }

    pub fn set_sub_bold(&self, bold: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-bold", bold);
        });
    }

    pub fn set_sub_color(&self, color: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-color", color.to_string());
        });
    }

    pub fn set_sub_border_color(&self, color: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-border-color", color.to_string());
        });
    }

    pub fn set_sub_shadow_color(&self, color: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-shadow-color", color.to_string());
        });
    }

    pub fn set_sub_border_size(&self, size: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-border-size", size);
        });
    }

    pub fn set_sub_shadow_offset(&self, offset: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-shadow-offset", offset);
        });
    }

    /// The person's own distance from the bottom; [`Self::set_sub_margin_lift`]
    /// adds to it.
    pub fn set_sub_margin_y(&self, val: i64) {
        self.sub_margin_base.store(val, Ordering::Relaxed);
        self.apply_sub_margin_y();
    }

    /// Raise text subtitles `lift` scaled pixels above the person's margin, clear
    /// of controls drawn over the bottom of the video; 0 puts them back.
    pub fn set_sub_margin_lift(&self, lift: i64) {
        self.sub_margin_lift.store(lift, Ordering::Relaxed);
        self.apply_sub_margin_y();
    }

    fn apply_sub_margin_y(&self) {
        let margin = self
            .sub_margin_base
            .load(Ordering::Relaxed)
            .saturating_add(self.sub_margin_lift.load(Ordering::Relaxed));
        self.with_mpv(|m| {
            let _ = m.set_property("sub-margin-y", margin);
        });
    }

    pub fn set_preferred_sub_lang(&self, lang: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("slang", lang.to_string());
        });
    }

    pub fn set_sub_auto(&self, mode: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-auto", mode.to_string());
        });
    }

    pub fn set_sub_border_style(&self, style: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-border-style", style.to_string());
        });
    }

    pub fn set_sub_scale_with_window(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-scale-with-window", enable);
        });
    }

    pub fn set_sub_ass_scale_with_window(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-ass-scale-with-window", enable);
        });
    }

    pub fn set_sub_filter_sdh(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-filter-sdh", enable);
        });
    }

    pub fn set_sub_filter_sdh_harder(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-filter-sdh-harder", enable);
        });
    }

    pub fn set_sub_gauss(&self, blur: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-gauss", blur);
        });
    }

    pub fn set_sub_gray(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-gray", enable);
        });
    }

    pub fn set_secondary_sid(&self, id: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("secondary-sid", id);
        });
    }

    /// Whether a secondary subtitle track is showing.
    pub fn has_secondary_sub(&self) -> bool {
        self.with_mpv(|m| {
            m.get_property::<i64>("secondary-sid")
                .is_ok_and(|id| id > 0)
        })
    }

    pub fn disable_secondary_sub(&self) {
        self.with_mpv(|m| {
            let _ = m.set_property("secondary-sid", false);
        });
    }
}
