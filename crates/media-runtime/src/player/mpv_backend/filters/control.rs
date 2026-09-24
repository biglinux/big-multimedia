use log::debug;

use crate::player::mpv_backend::MpvBackend;

pub(in crate::player::mpv_backend) fn software_hwdec(preference: &str) -> String {
    preference
        .split(',')
        .map(|mode| match mode {
            "no" | "auto-copy" | "auto-copy-safe" | "auto-copy-unsafe" => mode.to_string(),
            "auto" | "auto-safe" => "auto-copy".to_string(),
            "auto-unsafe" => "auto-copy-unsafe".to_string(),
            mode if mode.ends_with("-copy") => mode.to_string(),
            mode => format!("{mode}-copy"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

impl MpvBackend {
    pub fn vf_append(&self, label: &str, filter: &str) {
        self.update_video_filters("add", &format!("@{label}:lavfi=[{filter}]"));
    }

    pub fn vf_remove(&self, label: &str) {
        self.update_video_filters("remove", &format!("@{label}"));
    }

    pub fn get_vf(&self) -> String {
        self.with_mpv(|mpv| mpv.get_property::<String>("vf").unwrap_or_default())
    }

    pub fn set_vf(&self, vf: &str) {
        self.update_video_filters("set", vf);
    }

    fn update_video_filters(&self, operation: &str, value: &str) {
        self.with_mpv(|mpv| {
            let mut preference = self
                .hwdec_before_filters
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let starting = preference.is_none() && operation != "remove" && !value.is_empty();
            if starting {
                let original = mpv
                    .get_property::<String>("hwdec")
                    .unwrap_or_else(|_| "no".into());
                // Software effects need CPU-addressable frames, not CPU decoding.
                if mpv
                    .set_property("hwdec", software_hwdec(&original))
                    .is_err()
                {
                    return;
                }
                *preference = Some(original);
            }
            let result = mpv.command("vf", &[operation, value]);
            debug!("[VF] {operation}: {result:?}");
            let empty = mpv
                .get_property::<String>("vf")
                .is_ok_and(|vf| vf.is_empty());
            if (empty || (result.is_err() && starting))
                && let Some(original) = preference.take()
            {
                let _ = mpv.set_property("hwdec", original);
            }
        });
    }

    /// Micro-seek forces pipeline rebuild for live preview during playback.
    pub fn set_audio_filter_with_seek(&self, af: &str) {
        self.with_mpv(|mpv| {
            let _ = mpv.set_property("af", af.to_string());
            debug!("set_audio_filter_with_seek: af={af:?}");
            let _ = mpv.command("seek", &["0", "relative"]);
        });
    }
}
