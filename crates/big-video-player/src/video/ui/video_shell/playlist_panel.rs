//! The playlist and the folder browser, floating over the video on the left in
//! the settings panel's see-through card: it no longer squeezes the picture
//! aside. It opens from the header buttons or their shortcuts and closes the
//! same way, on Escape, or on a click on the video.

use super::*;

pub(super) struct PlaylistPanel {
    pub(super) revealer: gtk::Revealer,
    card: gtk::Box,
}

impl PlaylistPanel {
    pub(super) fn new(
        content: &impl IsA<gtk::Widget>,
        sender: &ComponentSender<VideoShell>,
    ) -> Self {
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(400)
            .css_classes(["settings-panel", "playlist-panel"])
            .build();
        content.set_vexpand(true);
        card.append(content);
        card.update_property(&[gtk::accessible::Property::Label(&i18n("Playlist"))]);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let sender = sender.clone();
            move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape {
                    sender.input(VideoShellInput::HidePlaylistPanel);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        });
        card.add_controller(keys);
        let revealer = gtk::Revealer::builder()
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Fill)
            .transition_type(gtk::RevealerTransitionType::SlideRight)
            .transition_duration(150)
            .margin_start(8)
            .child(&card)
            .build();
        Self { revealer, card }
    }

    pub(super) fn shows(&self) -> bool {
        self.revealer.reveals_child()
    }

    /// Show between the header and the controls, `top` and `bottom` pixels
    /// from the edges, or hide.
    pub(super) fn set_shown(&self, shown: bool, top: i32, bottom: i32) {
        if shown {
            self.revealer.set_margin_top(top);
            self.revealer.set_margin_bottom(bottom);
        }
        self.revealer.set_reveal_child(shown);
        if shown {
            // Into the list, on the playing row when there is one: the arrow
            // keys and Alt+Up/Down then act at once. Once the slide is done,
            // or the focus goes to what was on screen before the card.
            let card = self.card.downgrade();
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                let Some(card) = card.upgrade() else {
                    return;
                };
                match first_list(card.upcast_ref()) {
                    Some(list) => {
                        list.grab_focus();
                    }
                    None => {
                        card.child_focus(gtk::DirectionType::TabForward);
                    }
                }
            });
        }
    }
}

/// The first list or grid view under `widget`.
fn first_list(widget: &gtk::Widget) -> Option<gtk::Widget> {
    if widget.is::<gtk::ListView>() || widget.is::<gtk::GridView>() {
        return Some(widget.clone());
    }
    std::iter::successors(widget.first_child(), gtk::Widget::next_sibling)
        .find_map(|child| first_list(&child))
}
