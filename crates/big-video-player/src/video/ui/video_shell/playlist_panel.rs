//! The playlist and the folder browser, floating over the video on the left in
//! the settings panel's see-through card: it no longer squeezes the picture
//! aside. It opens from the header buttons or their shortcuts and closes the
//! same way, on Escape, or on a click on the video.

use super::*;

pub(super) struct PlaylistPanel {
    pub(super) revealer: gtk::Revealer,
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
        // The search swallows Escape, so the card hears it first while the
        // search has the focus: the first clears it, the next closes.
        let search_keys = gtk::EventControllerKey::new();
        search_keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        search_keys.connect_key_pressed({
            let sender = sender.clone();
            let card = card.downgrade();
            move |_, key, _, _| {
                let search = card
                    .upgrade()
                    .and_then(|card| card.root())
                    .and_then(|root| root.focus())
                    .and_then(|focus| focus.ancestor(gtk::SearchEntry::static_type()))
                    .and_downcast::<gtk::SearchEntry>();
                let Some(search) = search.filter(|_| key == gtk::gdk::Key::Escape) else {
                    return glib::Propagation::Proceed;
                };
                if search.text().is_empty() {
                    sender.input(VideoShellInput::HidePlaylistPanel);
                } else {
                    search.set_text("");
                }
                glib::Propagation::Stop
            }
        });
        card.add_controller(search_keys);
        let revealer = gtk::Revealer::builder()
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Fill)
            .transition_type(gtk::RevealerTransitionType::SlideRight)
            .transition_duration(150)
            .margin_start(8)
            .child(&card)
            .build();
        // Into the list, on the playing row when there is one: the arrow
        // keys and Alt+Up/Down then act at once. Once the slide is done, or
        // the focus goes to what was on screen before the card.
        revealer.connect_child_revealed_notify(|revealer| {
            if revealer.is_child_revealed()
                && let Some(card) = revealer.child()
            {
                focus_into(&card);
            }
        });
        Self { revealer }
    }

    pub(super) fn shows(&self) -> bool {
        self.revealer.reveals_child()
    }

    /// Show between the header and the controls, `top` and `bottom` pixels
    /// from the edges.
    pub(super) fn show(&self, top: i32, bottom: i32) {
        self.revealer.set_margin_top(top);
        self.revealer.set_margin_bottom(bottom);
        self.revealer.set_reveal_child(true);
    }

    pub(super) fn hide(&self) {
        self.revealer.set_reveal_child(false);
    }
}

/// Focus the list in `card`; without a list (the Files places), the search:
/// typing then searches instead of reaching the player's shortcuts.
fn focus_into(card: &gtk::Widget) {
    let target = first_of(card, &|widget| {
        widget.is::<gtk::ListView>() || widget.is::<gtk::GridView>()
    })
    .or_else(|| first_of(card, &|widget| widget.is::<gtk::SearchEntry>()));
    match target {
        Some(widget) => {
            widget.grab_focus();
        }
        None => {
            card.child_focus(gtk::DirectionType::TabForward);
        }
    }
}

/// The first widget under `widget`, itself included, that `wanted` accepts.
fn first_of(widget: &gtk::Widget, wanted: &dyn Fn(&gtk::Widget) -> bool) -> Option<gtk::Widget> {
    if wanted(widget) {
        return Some(widget.clone());
    }
    std::iter::successors(widget.first_child(), gtk::Widget::next_sibling)
        .find_map(|child| first_of(&child, wanted))
}
