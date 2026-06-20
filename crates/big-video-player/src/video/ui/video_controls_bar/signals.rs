use super::*;

/// Wire the buttons and scrub bar to output intents.
pub(super) fn wire_signals(
    sender: &ComponentSender<VideoControlsBar>,
    seek_bar: &gtk::Scale,
    prev_btn: &gtk::Button,
    play_btn: &gtk::Button,
    next_btn: &gtk::Button,
    fullscreen_btn: &gtk::Button,
    suppress_seek: &Rc<std::cell::Cell<bool>>,
) {
    let s = sender.clone();
    play_btn.connect_clicked(move |_| {
        let _ = s.output(VideoControlsBarOutput::TogglePlay);
    });
    let s = sender.clone();
    prev_btn.connect_clicked(move |_| {
        let _ = s.output(VideoControlsBarOutput::Previous);
    });
    let s = sender.clone();
    next_btn.connect_clicked(move |_| {
        let _ = s.output(VideoControlsBarOutput::Next);
    });
    let s = sender.clone();
    fullscreen_btn.connect_clicked(move |_| {
        let _ = s.output(VideoControlsBarOutput::ToggleFullscreen);
    });
    let s = sender.clone();
    let guard = suppress_seek.clone();
    seek_bar.connect_change_value(move |_scale, _scroll, value| {
        if !guard.get() {
            let _ = s.output(VideoControlsBarOutput::SeekChanged(value.clamp(0.0, 1.0)));
        }
        gtk::glib::Propagation::Proceed
    });
}
