//! The playlist and the folder browser, floating over the video on the left in
//! the settings panel's see-through card, without squeezing the picture
//! aside. It opens from the header buttons or their shortcuts and closes the
//! same way, on Escape, or on a click on the video.

use super::*;

const PANEL_WIDTH: i32 = 400;
const PANEL_MARGIN: i32 = 8;

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
            .width_request(PANEL_WIDTH)
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
        // One width for every page: a long folder path ellipsizes instead
        // of widening the card over the welcome screen's buttons.
        let clamp = adw::Clamp::builder()
            .maximum_size(PANEL_WIDTH)
            .tightening_threshold(PANEL_WIDTH)
            .child(&card)
            .build();
        let revealer = gtk::Revealer::builder()
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Fill)
            .transition_type(gtk::RevealerTransitionType::SlideRight)
            .transition_duration(150)
            .margin_start(PANEL_MARGIN)
            .child(&clamp)
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

    /// While the panel is open, place `widget`, an overlay child of
    /// `overlay`, in the room beside it.
    pub(super) fn keep_clear(&self, overlay: &gtk::Overlay, widget: &impl IsA<gtk::Widget>) {
        let widget = widget.upcast_ref::<gtk::Widget>().downgrade();
        let revealer = self.revealer.downgrade();
        overlay.connect_get_child_position(move |overlay, child| {
            let (widget, revealer) = (widget.upgrade()?, revealer.upgrade()?);
            if *child != widget || !revealer.reveals_child() {
                return None;
            }
            let (_, natural_width, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
            let (_, natural_height, _, _) =
                child.measure(gtk::Orientation::Vertical, natural_width);
            let (x, y) = beside_panel(
                overlay.width(),
                overlay.height(),
                natural_width,
                natural_height,
            );
            Some(gtk::gdk::Rectangle::new(
                x,
                y,
                natural_width.min(overlay.width()),
                natural_height.min(overlay.height()),
            ))
        });
    }
}

/// Where a `child_width` × `child_height` widget goes in a `width` × `height`
/// overlay beside the open panel: centered in the room right of it, or as far
/// right as it fits when that room is too narrow.
fn beside_panel(width: i32, height: i32, child_width: i32, child_height: i32) -> (i32, i32) {
    let edge = PANEL_MARGIN + PANEL_WIDTH;
    let x = (edge + (width - edge - child_width) / 2)
        .min(width - child_width)
        .max(0);
    (x, ((height - child_height) / 2).max(0))
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

#[cfg(test)]
mod tests {
    use super::beside_panel;

    #[test]
    fn beside_panel_centers_in_the_room_left() {
        // 1100 wide: the room right of the 408 px panel is 692 px.
        assert_eq!(beside_panel(1100, 600, 528, 200), (490, 200));
    }

    #[test]
    fn beside_panel_stops_at_the_right_edge() {
        assert_eq!(beside_panel(640, 400, 528, 200), (112, 100));
        assert_eq!(beside_panel(400, 300, 528, 400), (0, 0));
    }
}
