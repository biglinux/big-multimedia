//! Subtitle properties: appearance, colors, delay, language, SDH, secondary subs,
//! closed captions, image subs, border/scaling.

use super::MpvBackend;

impl MpvBackend {
    pub fn set_subtitle_track(&self, id: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sid", id);
        });
    }

    pub fn disable_subtitles(&self) {
        self.with_mpv(|m| {
            let _ = m.set_property("sid", false);
        });
    }

    pub fn add_subtitle_file(&self, path: &str) {
        self.with_mpv(|m| {
            let _ = m.command("sub-add", &[path, "auto"]);
        });
    }

    pub fn set_sub_delay(&self, delay: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-delay", delay);
        });
    }

    pub fn set_sub_font_size(&self, size: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-font-size", size);
        });
    }

    pub fn set_sub_font(&self, font: &str) {
        if !font.is_empty() {
            self.with_mpv(|m| {
                let _ = m.set_property("sub-font", font.to_string());
            });
        }
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

    pub fn set_sub_margin_y(&self, val: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-margin-y", val);
        });
    }

    pub fn set_preferred_sub_lang(&self, lang: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("slang", lang.to_string());
        });
    }

    pub fn set_preferred_audio_lang(&self, lang: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("alang", lang.to_string());
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

    pub fn disable_secondary_sub(&self) {
        self.with_mpv(|m| {
            let _ = m.set_property("secondary-sid", false);
        });
    }

    pub fn set_secondary_sub_visibility(&self, visible: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("secondary-sub-visibility", visible);
        });
    }

    pub fn get_secondary_sub_visibility(&self) -> bool {
        self.with_mpv(|m| Self::get_bool_property(m, "secondary-sub-visibility").unwrap_or(false))
    }

    pub fn set_secondary_sub_pos(&self, pos: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("secondary-sub-pos", pos);
        });
    }

    pub fn sub_seek(&self, dir: i64) {
        self.with_mpv(|m| {
            let _ = m.command("sub-seek", &[&dir.to_string()]);
        });
    }

    pub fn set_sub_fps(&self, fps: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("sub-fps", fps);
        });
    }

    pub fn get_sub_fps(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("sub-fps").unwrap_or(0.0))
    }

    /// `channel`: 0 = off, 1-4 = CC1-CC4.
    pub fn set_sub_codec_cc(&self, channel: i64) {
        if channel == 0 {
            self.with_mpv(|m| {
                let _ = m.set_property("sub-codec", String::new());
            });
        } else {
            self.with_mpv(|m| {
                let _ = m.set_property("sub-codec", format!("eia_608:{channel}"));
            });
        }
    }
}
