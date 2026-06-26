use super::*;

/// Wire the buttons and scrub bar to output intents.
pub(super) fn wire_signals(
    sender: &ComponentSender<VideoControlsBar>,
    seek_bar: &gtk::Scale,
    previous_button: &gtk::Button,
    play_button: &gtk::Button,
    next_button: &gtk::Button,
    fullscreen_button: &gtk::Button,
    suppress_seek: &Rc<std::cell::Cell<bool>>,
) {
    let play_sender = sender.clone();
    play_button.connect_clicked(move |_| {
        let _ = play_sender.output(VideoControlsBarOutput::TogglePlay);
    });
    let previous_sender = sender.clone();
    previous_button.connect_clicked(move |_| {
        let _ = previous_sender.output(VideoControlsBarOutput::Previous);
    });
    let next_sender = sender.clone();
    next_button.connect_clicked(move |_| {
        let _ = next_sender.output(VideoControlsBarOutput::Next);
    });
    let fullscreen_sender = sender.clone();
    fullscreen_button.connect_clicked(move |_| {
        let _ = fullscreen_sender.output(VideoControlsBarOutput::ToggleFullscreen);
    });
    let seek_sender = sender.clone();
    let suppress_seek_guard = suppress_seek.clone();
    seek_bar.connect_change_value(move |_scale, _scroll, value| {
        if !suppress_seek_guard.get() {
            let _ = seek_sender.output(VideoControlsBarOutput::SeekChanged(value.clamp(0.0, 1.0)));
        }
        gtk::glib::Propagation::Proceed
    });
}
