use super::*;

impl VideoShell {
    /// Shared mpv handle (for `menu_dialogs` to drive playback).
    pub(crate) fn mpv_handle(&self) -> &Rc<RefCell<Option<Arc<MpvBackend>>>> {
        &self.mpv
    }

    /// Liveness flag a queued screenshot checks before publishing its file;
    /// cleared on shutdown so a late write cannot outlive the shell.
    pub(crate) fn screenshot_owner(&self) -> &Arc<std::sync::atomic::AtomicBool> {
        &self.screenshot_owner
    }

    /// Where playback is, on the TV when one plays; 0 with nothing loaded.
    pub(crate) fn playback_position(&self) -> f64 {
        match self.tv.attached() {
            Some(state) => state.position,
            None => self
                .mpv
                .borrow()
                .as_ref()
                .map_or(0.0, |mpv| mpv.get_position()),
        }
    }

    /// Player settings (for the sleep-timer dialog persistence).
    pub(crate) fn settings_handle(&self) -> &Settings {
        &self.settings
    }

    /// Show `mode` in the open panel (or open it there), with its header
    /// button pressed and the other one released.
    pub(crate) fn show_sidebar_mode(&self, mode: SidebarMode) {
        if !(self.playlist_panel.shows() && self.sidebar_mode.get() == mode) {
            self.toggle_sidebar_mode(mode);
        }
    }

    /// Toggle a media sidebar mode from keyboard/GAction without depending on
    /// the auto-hidden OSD header buttons being visible.
    pub(crate) fn toggle_sidebar_mode(&self, mode: SidebarMode) {
        let is_same_visible = self.playlist_panel.shows() && self.sidebar_mode.get() == mode;
        self.sidebar_toggle_updating.set(true);
        self.files_sidebar_button
            .set_active(!is_same_visible && mode == SidebarMode::Files);
        self.playlist_sidebar_button
            .set_active(!is_same_visible && mode == SidebarMode::Playlist);
        self.sidebar_toggle_updating.set(false);

        if is_same_visible {
            self.playlist_panel.hide();
            return;
        }

        self.sidebar_mode.set(mode);
        self.playlist.emit(VideoBrowserSidebarInput::SetMode(mode));
        let (top, bottom) = self.panel_edges();
        self.playlist_panel.show(top, bottom);
    }
}
