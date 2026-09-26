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
        // The converter reads the colours as the player was told to.
        let colours = self.video_transform.source_colours();
        let source_hdr =
            (program == converter_handoff::VIDEO_CONVERTER && colours != "auto").then_some(colours);
        self.toast(&converter_handoff::open(
            program,
            &[input],
            cuts,
            source_hdr,
        ));
    }
}
