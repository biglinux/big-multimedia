use log::debug;

use crate::player::mpv_backend::MpvBackend;

impl MpvBackend {
    pub fn vf_append(&self, label: &str, filter: &str) {
        self.with_mpv(|mpv| {
            let _ = mpv.set_property("hwdec", "no".to_string());
        });
        self.with_mpv(|mpv| {
            let tagged = format!("@{label}:lavfi-{filter}");
            debug!("[VF] vf add {tagged:?}");
            let result = mpv.command("vf", &["add", &tagged]);
            debug!("[VF] vf add result: {result:?}");
            let vf: String = mpv.get_property("vf").unwrap_or_default();
            debug!("[VF] current vf: {vf:?}");
        });
    }

    pub fn vf_remove(&self, label: &str) {
        self.with_mpv(|mpv| {
            let tagged = format!("@{label}");
            debug!("[VF] vf remove {tagged:?}");
            let result = mpv.command("vf", &["remove", &tagged]);
            debug!("[VF] vf remove result: {result:?}");
            let vf: String = mpv.get_property("vf").unwrap_or_default();
            debug!("[VF] current vf after remove: {vf:?}");
            if vf.is_empty() {
                debug!("[VF] restoring hwdec=auto-safe");
                let _ = mpv.set_property("hwdec", "auto-safe".to_string());
            }
        });
    }

    pub fn get_vf(&self) -> String {
        self.with_mpv(|mpv| mpv.get_property::<String>("vf").unwrap_or_default())
    }

    pub fn set_vf(&self, vf: &str) {
        self.with_mpv(|mpv| {
            let _ = mpv.set_property("vf", vf.to_string());
        });
    }

    pub fn restore_hwdec(&self) {
        self.with_mpv(|mpv| {
            let _ = mpv.set_property("hwdec", "auto-safe".to_string());
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

    pub fn af_append(&self, filter: &str) {
        self.with_mpv(|mpv| {
            let _ = mpv.command("af", &["add", filter]);
        });
    }

    pub fn af_remove(&self, filter: &str) {
        self.with_mpv(|mpv| {
            let _ = mpv.command("af", &["remove", filter]);
        });
    }
}
