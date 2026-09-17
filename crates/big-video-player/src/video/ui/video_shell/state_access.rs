use super::*;

impl VideoShell {
    /// Toast host (for `menu_dialogs` feedback).
    pub(crate) fn toast_overlay(&self) -> &adw::ToastOverlay {
        &self.toast_overlay
    }

    /// Shared mpv handle (for `menu_dialogs` to drive playback).
    pub(crate) fn mpv_handle(&self) -> &Rc<RefCell<Option<Arc<MpvBackend>>>> {
        &self.mpv
    }

    /// Liveness flag a queued screenshot checks before publishing its file;
    /// cleared on shutdown so a late write cannot outlive the shell.
    pub(crate) fn screenshot_owner(&self) -> &Arc<std::sync::atomic::AtomicBool> {
        &self.screenshot_owner
    }

    /// Player settings (for the sleep-timer dialog persistence).
    pub(crate) fn settings_handle(&self) -> &Settings {
        &self.settings
    }

    /// True while a sleep-timer countdown source is armed.
    pub(crate) fn sleep_timer_is_armed(&self) -> bool {
        self.sleep_timer_source.is_some()
    }

    /// Seconds left on the armed sleep timer.
    pub(crate) fn sleep_timer_remaining_secs(&self) -> u32 {
        self.sleep_timer_remaining
    }

    /// Pre-fade volume captured when the timer was armed.
    pub(crate) fn sleep_timer_volume_value(&self) -> f64 {
        self.sleep_timer_volume
    }

    /// Replace the stored sleep-timer countdown source.
    pub(crate) fn set_sleep_timer_source(&mut self, source: Option<glib::SourceId>) {
        self.sleep_timer_source = source;
    }

    /// Take (and clear) the stored sleep-timer countdown source.
    pub(crate) fn take_sleep_timer_source(&mut self) -> Option<glib::SourceId> {
        self.sleep_timer_source.take()
    }

    /// Set the sleep-timer remaining seconds.
    pub(crate) fn set_sleep_timer_remaining(&mut self, secs: u32) {
        self.sleep_timer_remaining = secs;
    }

    /// Set the on-expiry sleep-timer action id.
    pub(crate) fn set_sleep_timer_action(&mut self, action: String) {
        self.sleep_timer_action = action;
    }

    /// Take (and clear) the on-expiry sleep-timer action id.
    pub(crate) fn take_sleep_timer_action(&mut self) -> String {
        std::mem::take(&mut self.sleep_timer_action)
    }

    /// Set the pre-fade sleep-timer volume.
    pub(crate) fn set_sleep_timer_volume(&mut self, volume: f64) {
        self.sleep_timer_volume = volume;
    }

    /// Toggle a media sidebar mode from keyboard/GAction without depending on
    /// the auto-hidden OSD header buttons being visible.
    pub(crate) fn toggle_sidebar_mode(&self, mode: SidebarMode) {
        let is_same_visible = self.split_view.shows_sidebar() && self.sidebar_mode.get() == mode;
        self.sidebar_toggle_updating.set(true);
        self.files_sidebar_button
            .set_active(!is_same_visible && mode == SidebarMode::Files);
        self.playlist_sidebar_button
            .set_active(!is_same_visible && mode == SidebarMode::Playlist);
        self.sidebar_toggle_updating.set(false);

        if is_same_visible {
            self.split_view.set_show_sidebar(false);
            return;
        }

        self.sidebar_mode.set(mode);
        self.playlist.emit(VideoBrowserSidebarInput::SetMode(mode));
        self.split_view.set_show_sidebar(true);
    }
}
