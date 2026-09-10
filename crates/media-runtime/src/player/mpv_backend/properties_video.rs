//! Video adjustments: contrast, brightness, gamma, saturation, hue, zoom, pan, aspect,
//! rotate, flip, panscan, crop, deinterlace, sync, interpolation, HDR, tone mapping,
//! shaders.

use super::MpvBackend;

impl MpvBackend {
    pub fn set_contrast(&self, val: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("contrast", val);
        });
    }
    pub fn set_brightness(&self, val: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("brightness", val);
        });
    }
    pub fn set_gamma(&self, val: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("gamma", val);
        });
    }
    pub fn set_saturation(&self, val: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("saturation", val);
        });
    }
    pub fn set_hue(&self, val: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("hue", val);
        });
    }
    pub fn get_contrast(&self) -> i64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property("contrast").unwrap_or(0)
    }
    pub fn get_brightness(&self) -> i64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property("brightness").unwrap_or(0)
    }
    pub fn get_gamma(&self) -> i64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property("gamma").unwrap_or(0)
    }
    pub fn get_saturation(&self) -> i64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property("saturation").unwrap_or(0)
    }
    pub fn get_hue(&self) -> i64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property("hue").unwrap_or(0)
    }

    pub fn set_video_zoom(&self, val: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("video-zoom", val);
        });
    }
    pub fn set_video_aspect(&self, val: f64) {
        self.with_mpv(|m| {
            if val < 0.0 {
                let _ = m.set_property("video-aspect-override", -1.0_f64);
            } else {
                let _ = m.set_property("video-aspect-override", val);
            }
        });
    }

    pub fn set_video_rotate(&self, degrees: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("video-rotate", degrees);
        });
    }

    pub fn get_video_rotate(&self) -> i64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property("video-rotate").unwrap_or(0)
    }

    pub fn set_video_flip(&self, hflip: bool, vflip: bool) {
        let mut filters = Vec::new();
        if hflip {
            filters.push("hflip");
        }
        if vflip {
            filters.push("vflip");
        }
        self.with_mpv(|m| {
            if filters.is_empty() {
                let _ = m.set_property("vf", String::new());
                let _ = m.set_property("hwdec", "auto-safe".to_string());
            } else {
                // vf must be set first — mpv auto-disables hwdec when sw filters needed.
                let _ = m.set_property("vf", filters.join(","));
                let _ = m.set_property("hwdec", "no".to_string());
            }
        });
    }

    pub fn get_video_zoom(&self) -> f64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property("video-zoom").unwrap_or(0.0)
    }

    pub fn get_video_pan_x(&self) -> f64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property("video-pan-x").unwrap_or(0.0)
    }

    pub fn get_video_pan_y(&self) -> f64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property("video-pan-y").unwrap_or(0.0)
    }

    pub fn set_video_pan_x(&self, val: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("video-pan-x", val);
        });
    }

    pub fn set_video_pan_y(&self, val: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("video-pan-y", val);
        });
    }

    pub fn set_panscan(&self, val: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("panscan", val);
        });
    }

    pub fn get_panscan(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("panscan").unwrap_or(0.0))
    }

    pub fn set_video_crop(&self, crop: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("video-crop", crop.to_string());
        });
    }

    pub fn set_deinterlace(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "deinterlace",
                if enable {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    pub fn get_deinterlace(&self) -> bool {
        self.with_mpv(|m| {
            m.get_property::<String>("deinterlace")
                .is_ok_and(|v| v == "yes")
        })
    }

    pub fn set_video_sync(&self, mode: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("video-sync", mode.to_string());
        });
    }

    pub fn set_interpolation(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("interpolation", enable);
        });
    }

    pub fn set_tscale(&self, filter: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("tscale", filter.to_string());
        });
    }

    pub fn set_tone_mapping(&self, algo: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("tone-mapping", algo.to_string());
        });
    }

    pub fn set_target_peak(&self, nits: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("target-peak", nits);
        });
    }

    pub fn set_target_contrast(&self, contrast: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("target-contrast", contrast);
        });
    }

    pub fn set_hdr_contrast_recovery(&self, strength: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("hdr-contrast-recovery", strength);
        });
    }

    pub fn set_hdr_compute_peak(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("hdr-compute-peak", enable);
        });
    }

    pub fn set_target_colorspace_hint(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("target-colorspace-hint", enable);
        });
    }

    pub fn set_gamut_mapping_mode(&self, mode: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("gamut-mapping-mode", mode.to_string());
        });
    }

    pub fn set_glsl_shaders(&self, shaders: &str) {
        log::info!("Setting glsl-shaders: {shaders:?}");
        self.with_mpv(|m| {
            let result = m.set_property("glsl-shaders", shaders.to_string());
            match result {
                Ok(()) => {
                    let actual = m.get_property::<String>("glsl-shaders").unwrap_or_default();
                    log::info!("glsl-shaders confirmed: {actual:?}");
                }
                Err(e) => log::error!("Failed to set glsl-shaders: {e}"),
            }
        });
    }

    pub fn get_glsl_shaders(&self) -> String {
        self.with_mpv(|m| m.get_property::<String>("glsl-shaders").unwrap_or_default())
    }

    /// Colon-separated on Unix. No-op if already present.
    pub fn glsl_shader_append(&self, path: &str) {
        let current = self.get_glsl_shaders();
        let new = shader_list_append(&current, path);
        if new != current {
            self.set_glsl_shaders(&new);
        }
    }

    pub fn glsl_shader_remove(&self, path: &str) {
        let current = self.get_glsl_shaders();
        let new = shader_list_remove(&current, path);
        self.set_glsl_shaders(&new);
    }

    pub fn glsl_shader_remove_prefix(&self, prefix: &str) {
        let current = self.get_glsl_shaders();
        let new = shader_list_remove_prefix(&current, prefix);
        self.set_glsl_shaders(&new);
    }
}

/// Append `path` to colon-separated list. No-op if already present.
pub(crate) fn shader_list_append(list: &str, path: &str) -> String {
    let paths: Vec<&str> = list.split(':').filter(|s| !s.is_empty()).collect();
    if paths.contains(&path) {
        return list.to_string();
    }
    let mut new_paths = paths;
    new_paths.push(path);
    new_paths.join(":")
}

pub(crate) fn shader_list_remove(list: &str, path: &str) -> String {
    list.split(':')
        .filter(|s| !s.is_empty() && *s != path)
        .collect::<Vec<_>>()
        .join(":")
}

pub(crate) fn shader_list_remove_prefix(list: &str, prefix: &str) -> String {
    list.split(':')
        .filter(|s| !s.is_empty() && !s.starts_with(prefix))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_list_append_to_empty() {
        assert_eq!(shader_list_append("", "/a.glsl"), "/a.glsl");
    }

    #[test]
    fn test_shader_list_append_to_existing() {
        assert_eq!(shader_list_append("/a.glsl", "/b.glsl"), "/a.glsl:/b.glsl");
    }

    #[test]
    fn test_shader_list_append_duplicate_noop() {
        let list = "/a.glsl:/b.glsl";
        assert_eq!(shader_list_append(list, "/a.glsl"), list);
    }

    #[test]
    fn test_shader_list_append_multiple() {
        let list = "/a.glsl:/b.glsl";
        assert_eq!(
            shader_list_append(list, "/c.glsl"),
            "/a.glsl:/b.glsl:/c.glsl"
        );
    }

    #[test]
    fn test_shader_list_remove_existing() {
        assert_eq!(
            shader_list_remove("/a.glsl:/b.glsl:/c.glsl", "/b.glsl"),
            "/a.glsl:/c.glsl"
        );
    }

    #[test]
    fn test_shader_list_remove_not_present() {
        assert_eq!(
            shader_list_remove("/a.glsl:/b.glsl", "/z.glsl"),
            "/a.glsl:/b.glsl"
        );
    }

    #[test]
    fn test_shader_list_remove_single_shader_path() {
        assert_eq!(shader_list_remove("/a.glsl", "/a.glsl"), "");
    }

    #[test]
    fn test_shader_list_remove_empty_list() {
        assert_eq!(shader_list_remove("", "/a.glsl"), "");
    }

    #[test]
    fn test_shader_list_remove_partial_match_not_removed() {
        assert_eq!(
            shader_list_remove("/a.glsl:/a.glsl.bak", "/a.glsl"),
            "/a.glsl.bak"
        );
    }

    #[test]
    fn test_shader_list_remove_prefix_matches() {
        let list = "/usr/share/shaders/fsr.glsl:/usr/share/shaders/cas.glsl:/home/user/my.glsl";
        assert_eq!(
            shader_list_remove_prefix(list, "/usr/share/shaders/"),
            "/home/user/my.glsl"
        );
    }

    #[test]
    fn test_shader_list_remove_prefix_no_match() {
        let list = "/a.glsl:/b.glsl";
        assert_eq!(
            shader_list_remove_prefix(list, "/nothing/"),
            "/a.glsl:/b.glsl"
        );
    }

    #[test]
    fn test_shader_list_remove_prefix_all_removed() {
        let list = "/usr/a.glsl:/usr/b.glsl";
        assert_eq!(shader_list_remove_prefix(list, "/usr/"), "");
    }

    #[test]
    fn test_shader_list_remove_prefix_empty_list() {
        assert_eq!(shader_list_remove_prefix("", "/prefix/"), "");
    }

    #[test]
    fn test_shader_list_remove_prefix_empty_prefix_removes_all() {
        assert_eq!(shader_list_remove_prefix("/a.glsl:/b.glsl", ""), "");
    }
}
