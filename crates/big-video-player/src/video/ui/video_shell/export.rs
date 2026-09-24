//! Edit & Export menu actions: hand the playing file to a converter GUI. The
//! player converts nothing itself; [`super::trim`] adds the cuts marked here.

use super::*;

impl VideoShell {
    /// Open the playing local file in `program`, keeping only `cuts` when any.
    pub(in crate::video::ui::video_shell) fn open_in_converter(
        &self,
        program: &str,
        cuts: &[(f64, f64)],
    ) {
        let Some(input) = self
            .mpv
            .borrow()
            .as_ref()
            .and_then(|mpv| mpv.get_current_file())
        else {
            self.toast(&i18n("No file is playing"));
            return;
        };
        if super::playback::is_remote_uri(&input) {
            self.toast(&i18n("Cannot export a network stream"));
            return;
        }
        self.launch_converter(program, &[input], cuts);
    }

    /// Spawn `program` with `paths` (and `cuts`) and toast the outcome.
    pub(in crate::video::ui::video_shell) fn launch_converter(
        &self,
        program: &str,
        paths: &[String],
        cuts: &[(f64, f64)],
    ) {
        match converter_handoff::open(program, paths, cuts) {
            Ok(()) => self.toast(&i18n("Opening in converter…")),
            Err(error) => {
                log::warn!("could not launch {program}: {error}");
                self.toast(&i18n("Could not launch the converter"));
            }
        }
    }
}
