//! Video adjustments: contrast, brightness, gamma, saturation, hue, zoom, pan, aspect,
//! rotate, flip, panscan, crop, deinterlace, sync, interpolation, HDR, tone mapping,
//! shaders.

use super::MpvBackend;

/// mpv's description of a video's frames, empty or 0 where it gives none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoParams {
    pub pixel_format: String,
    /// The surface's real format under hardware decoding.
    pub hw_pixel_format: String,
    pub colormatrix: String,
    pub primaries: String,
    /// The transfer function, which mpv calls gamma.
    pub gamma: String,
    pub width: f64,
}

impl MpvBackend {
    /// The frames the decoder produces (`video-dec-params`), as the file
    /// describes them.
    pub fn decoder_params(&self) -> VideoParams {
        self.read_video_params("video-dec-params")
    }

    /// The frames as mpv outputs them (`video-params`), after its filters.
    pub fn output_params(&self) -> VideoParams {
        self.read_video_params("video-params")
    }

    fn read_video_params(&self, prefix: &str) -> VideoParams {
        self.with_mpv(|m| {
            let param = |key: &str| {
                m.get_property::<String>(&format!("{prefix}/{key}"))
                    .unwrap_or_default()
            };
            VideoParams {
                pixel_format: param("pixelformat"),
                hw_pixel_format: param("hw-pixelformat"),
                colormatrix: param("colormatrix"),
                primaries: param("primaries"),
                gamma: param("gamma"),
                width: m.get_property::<f64>(&format!("{prefix}/w")).unwrap_or(0.0),
            }
        })
    }

    /// The source video's size before scaling, when one is loaded.
    pub fn video_source_size(&self) -> Option<(i64, i64)> {
        self.with_mpv(|m| {
            let w = m.get_property::<i64>("width").ok()?;
            let h = m.get_property::<i64>("height").ok()?;
            (w > 0 && h > 0).then_some((w, h))
        })
    }

    /// Frames per second after the video filters, 0 when unknown.
    pub fn estimated_fps(&self) -> f64 {
        self.get_property_f64("estimated-vf-fps")
    }

    /// The hardware decoder in use, empty when decoding in software.
    pub fn hwdec_current(&self) -> String {
        self.get_property_string("hwdec-current")
    }

    pub fn set_contrast(&self, val: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("contrast", val);
        });
    }
    pub fn set_brightness(&self, val: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("brightness", val);
        });
    }
    pub fn set_gamma(&self, val: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("gamma", val);
        });
    }
    pub fn set_saturation(&self, val: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("saturation", val);
        });
    }
    pub fn set_hue(&self, val: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("hue", val);
        });
    }
    pub fn get_contrast(&self) -> i64 {
        self.with_mpv(|m| m.get_property("contrast").unwrap_or(0))
    }
    pub fn get_brightness(&self) -> i64 {
        self.with_mpv(|m| m.get_property("brightness").unwrap_or(0))
    }
    pub fn get_gamma(&self) -> i64 {
        self.with_mpv(|m| m.get_property("gamma").unwrap_or(0))
    }
    pub fn get_saturation(&self) -> i64 {
        self.with_mpv(|m| m.get_property("saturation").unwrap_or(0))
    }
    pub fn get_hue(&self) -> i64 {
        self.with_mpv(|m| m.get_property("hue").unwrap_or(0))
    }

    pub fn set_video_zoom(&self, val: f64) {
        self.with_mpv(|m| {
            m.set_property_logged("video-zoom", val);
        });
    }

    pub fn set_video_rotate(&self, degrees: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("video-rotate", degrees);
        });
    }

    pub fn get_video_rotate(&self) -> i64 {
        self.with_mpv(|m| m.get_property("video-rotate").unwrap_or(0))
    }

    /// The `vf` label of the filter [`Self::set_video_flip`] owns.
    pub const FLIP_VF_LABEL: &str = "big-flip";

    pub fn set_video_flip(&self, hflip: bool, vflip: bool) {
        match (hflip, vflip) {
            (false, false) => self.vf_remove(Self::FLIP_VF_LABEL),
            (true, false) => self.vf_append(Self::FLIP_VF_LABEL, "hflip"),
            (false, true) => self.vf_append(Self::FLIP_VF_LABEL, "vflip"),
            (true, true) => self.vf_append(Self::FLIP_VF_LABEL, "hflip,vflip"),
        }
    }

    pub fn get_video_zoom(&self) -> f64 {
        self.with_mpv(|m| m.get_property("video-zoom").unwrap_or(0.0))
    }

    pub fn set_panscan(&self, val: f64) {
        self.with_mpv(|m| {
            m.set_property_logged("panscan", val);
        });
    }

    pub fn get_panscan(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("panscan").unwrap_or(0.0))
    }

    pub fn video_crop(&self) -> String {
        self.get_property_string("video-crop")
    }

    pub fn set_video_crop(&self, crop: &str) {
        self.with_mpv(|m| {
            m.set_property_logged("video-crop", crop.to_string());
        });
    }

    pub fn deinterlace(&self) -> bool {
        self.get_property_bool("deinterlace")
    }

    pub fn set_deinterlace(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("deinterlace", if enable { "yes" } else { "no" });
        });
    }

    /// Read the picture's colors as `mode` says: "pq" (HDR10), "hlg" or
    /// "sdr"; anything else trusts the file. A recording can describe no
    /// colors at all (a DVB sample: nothing in the stream, nothing per
    /// frame), and its HDR then plays grey and washed out. mpv's own format
    /// filter only relabels the frames, so decoding stays on the GPU.
    pub const SOURCE_COLORS_VF_LABEL: &str = "source-colors";

    pub fn set_source_colors(&self, mode: &str) {
        let format = match mode {
            "pq" => "format=gamma=pq:primaries=bt.2020:colormatrix=bt.2020-ncl",
            "hlg" => "format=gamma=hlg:primaries=bt.2020:colormatrix=bt.2020-ncl",
            "sdr" => "format=gamma=bt.1886:primaries=bt.709:colormatrix=bt.709",
            _ => "",
        };
        self.with_mpv(|m| {
            let label = Self::SOURCE_COLORS_VF_LABEL;
            let _ = m.command("vf", &["remove", &format!("@{label}")]);
            if !format.is_empty() {
                let _ = m.command("vf", &["add", &format!("@{label}:{format}")]);
            }
        });
    }

    pub fn set_tone_mapping(&self, algo: &str) {
        self.with_mpv(|m| {
            m.set_property_logged("tone-mapping", algo.to_string());
        });
    }

    /// 0 is mpv's `auto`, which the option takes only as text.
    pub fn set_target_peak(&self, nits: i64) {
        let value = if nits == 0 {
            "auto".to_string()
        } else {
            nits.to_string()
        };
        self.with_mpv(|m| {
            m.set_property_logged("target-peak", value);
        });
    }

    pub fn set_hdr_compute_peak(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("hdr-compute-peak", enable);
        });
    }

    pub fn set_gamut_mapping_mode(&self, mode: &str) {
        self.with_mpv(|m| {
            m.set_property_logged("gamut-mapping-mode", mode.to_string());
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
    fn append_to_empty_list() {
        assert_eq!(shader_list_append("", "/a.glsl"), "/a.glsl");
    }

    #[test]
    fn append_to_existing_list() {
        assert_eq!(shader_list_append("/a.glsl", "/b.glsl"), "/a.glsl:/b.glsl");
    }

    #[test]
    fn append_skips_duplicate_path() {
        let list = "/a.glsl:/b.glsl";
        assert_eq!(shader_list_append(list, "/a.glsl"), list);
    }

    #[test]
    fn append_keeps_existing_order() {
        let list = "/a.glsl:/b.glsl";
        assert_eq!(
            shader_list_append(list, "/c.glsl"),
            "/a.glsl:/b.glsl:/c.glsl"
        );
    }

    #[test]
    fn remove_prefix_drops_matching_paths() {
        let list = "/usr/share/shaders/fsr.glsl:/usr/share/shaders/cas.glsl:/home/user/my.glsl";
        assert_eq!(
            shader_list_remove_prefix(list, "/usr/share/shaders/"),
            "/home/user/my.glsl"
        );
    }

    #[test]
    fn remove_prefix_keeps_other_paths() {
        let list = "/a.glsl:/b.glsl";
        assert_eq!(
            shader_list_remove_prefix(list, "/nothing/"),
            "/a.glsl:/b.glsl"
        );
    }

    #[test]
    fn remove_prefix_can_empty_list() {
        let list = "/usr/a.glsl:/usr/b.glsl";
        assert_eq!(shader_list_remove_prefix(list, "/usr/"), "");
    }

    #[test]
    fn remove_prefix_on_empty_list() {
        assert_eq!(shader_list_remove_prefix("", "/prefix/"), "");
    }

    #[test]
    fn empty_prefix_removes_all_shaders() {
        assert_eq!(shader_list_remove_prefix("/a.glsl:/b.glsl", ""), "");
    }
}

#[cfg(all(test, not(miri)))]
mod flip_tests {
    use super::MpvBackend;

    #[test]
    fn flip_preserves_other_filters_and_hardware_preference() {
        let (sender, _events) = async_channel::unbounded();
        let backend = MpvBackend::new_audio_only_with_output(sender, "").unwrap();
        for preference in ["no", "auto-safe", "nvdec-copy"] {
            backend.set_hwdec(preference);
            let expected = backend.get_hwdec();
            backend.vf_append("picture", "eq=contrast=1.2");
            for (horizontal, vertical) in
                [(true, false), (true, true), (false, true), (false, false)]
            {
                backend.set_video_flip(horizontal, vertical);
                let filters = backend.get_vf();
                assert!(filters.contains("picture"), "lost contrast: {filters}");
                assert_eq!(filters.contains("hflip"), horizontal, "{filters}");
                assert_eq!(filters.contains("vflip"), vertical, "{filters}");
                assert_eq!(backend.get_hwdec(), expected);
            }
            backend.vf_remove("picture");
            assert_eq!(backend.get_hwdec(), expected);
            assert!(backend.get_vf().is_empty());
        }
    }
    #[test]
    fn hardware_choice_changed_during_effects_survives_removal() {
        let (sender, _events) = async_channel::unbounded();
        let backend = MpvBackend::new_audio_only_with_output(sender, "").unwrap();
        backend.set_hwdec("auto-safe");
        backend.vf_append("effect", "hflip");
        assert_eq!(
            backend.with_mpv(|mpv| mpv.get_property::<String>("hwdec").unwrap()),
            "auto-copy"
        );
        backend.set_hwdec("no");
        backend.vf_remove("effect");
        assert_eq!(backend.get_hwdec(), "no");
    }
}
