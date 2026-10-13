//! The card shown over a player while a TV plays what it sent.

use std::rc::Rc;

use gtk::prelude::*;

use super::receiver_line;
use crate::cast::daemon::State;
use crate::i18n::i18n;

/// What plays and where, for the card: the player may show something else.
fn card_line(state: &State) -> String {
    let receiver = receiver_line(state);
    if state.title.is_empty() {
        receiver
    } else {
        format!("{}\n{receiver}", state.title)
    }
}

/// What a [`TvCard`] button asks of its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TvCardAction {
    /// "Stop sending".
    Stop,
    /// "Use the player" (the TV keeps playing and the player goes its own
    /// way), or "Control the TV", its way back.
    ToggleRelease,
    /// "Show this one on the TV": the TV plays what the player holds instead.
    Replace,
    /// "Play the TV's here": the player opens what the TV plays, where the TV
    /// is.
    Follow,
    /// "Watch here too", or "Only on the TV".
    Here,
}

/// The card shown over a player while a TV plays its session: what is
/// happening, where, and the way out. The owner shows and fills it.
pub struct TvCard {
    pub root: gtk::Box,
    detail: gtk::Label,
    title: gtk::Label,
    /// "Use the player", or "Control the TV" once the player was released.
    release: gtk::Button,
    /// "Show this one on the TV" and "Play the TV's here": offered while
    /// released, when the player holds something other than what the TV
    /// plays.
    swap: Option<gtk::Box>,
    /// "Watch here too", or "Only on the TV" while the player follows the TV.
    here: Option<gtk::Button>,
}

impl TvCard {
    /// `act` runs for every button. `css` names the owner's card style;
    /// `strip` lays it out as one line, for a player whose middle holds a
    /// list that must stay usable. `swap` offers Replace and Follow once
    /// released; `here`, for a player that can show the video too, offers
    /// Here.
    pub fn new(
        css: &str,
        strip: bool,
        swap: bool,
        here: bool,
        act: impl Fn(TvCardAction) + 'static,
    ) -> Self {
        let act = Rc::new(act);
        let on_click = |button: &gtk::Button, action: TvCardAction| {
            let act = act.clone();
            button.connect_clicked(move |_| act(action));
        };
        let icon = gtk::Image::builder()
            .icon_name("tv-symbolic")
            .pixel_size(if strip { 24 } else { 48 })
            .build();
        let justify = if strip {
            gtk::Justification::Left
        } else {
            gtk::Justification::Center
        };
        let title = gtk::Label::builder()
            .label(i18n("Playing on the TV"))
            .css_classes([if strip { "heading" } else { "title-3" }])
            .wrap(true)
            .xalign(if strip { 0.0 } else { 0.5 })
            .justify(justify)
            .build();
        let detail = gtk::Label::builder()
            .css_classes(["dim-label"])
            .wrap(true)
            .xalign(if strip { 0.0 } else { 0.5 })
            .justify(justify)
            .build();
        let stop_button = gtk::Button::builder()
            .label(i18n("Stop sending"))
            .css_classes(["pill", "destructive-action"])
            .build();
        on_click(&stop_button, TvCardAction::Stop);
        let release_button = gtk::Button::builder()
            .label(i18n("Use the player"))
            .tooltip_text(i18n(
                "The TV keeps playing, and the player is free for this or another video.",
            ))
            .css_classes(["pill"])
            .build();
        on_click(&release_button, TvCardAction::ToggleRelease);
        let swap = swap.then(|| {
            let replace_button = gtk::Button::builder()
                .label(i18n("Show this one on the TV"))
                .tooltip_text(i18n(
                    "The TV plays what is in the player now, from where it is.",
                ))
                .css_classes(["pill", "suggested-action"])
                .build();
            on_click(&replace_button, TvCardAction::Replace);
            let follow_button = gtk::Button::builder()
                .label(i18n("Play the TV's here"))
                .tooltip_text(i18n(
                    "The player opens what the TV plays, at the same point.",
                ))
                .css_classes(["pill"])
                .build();
            on_click(&follow_button, TvCardAction::Follow);
            let swap = gtk::Box::builder()
                .spacing(8)
                .halign(gtk::Align::Center)
                .valign(gtk::Align::Center)
                .margin_top(if strip { 0 } else { 6 })
                .visible(false)
                .build();
            swap.append(&replace_button);
            swap.append(&follow_button);
            swap
        });
        let buttons = gtk::Box::builder()
            .spacing(8)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .margin_top(if strip { 0 } else { 6 })
            .build();
        let here = here.then(|| {
            let button = gtk::Button::builder().css_classes(["pill"]).build();
            on_click(&button, TvCardAction::Here);
            buttons.append(&button);
            button
        });
        buttons.append(&release_button);
        buttons.append(&stop_button);
        let root = gtk::Box::builder()
            .visible(false)
            .css_classes([css])
            .build();
        let inner = if strip {
            let words = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .valign(gtk::Align::Center)
                .hexpand(true)
                .build();
            words.append(&title);
            words.append(&detail);
            let inner = gtk::Box::builder()
                .spacing(12)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(12)
                .margin_end(12)
                .hexpand(true)
                .build();
            inner.append(&icon);
            inner.append(&words);
            if let Some(swap) = &swap {
                inner.append(swap);
            }
            inner.append(&buttons);
            inner
        } else {
            root.set_halign(gtk::Align::Center);
            root.set_valign(gtk::Align::Center);
            let inner = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .margin_top(18)
                .margin_bottom(18)
                .margin_start(24)
                .margin_end(24)
                .build();
            inner.append(&icon);
            inner.append(&title);
            inner.append(&detail);
            if let Some(swap) = &swap {
                inner.append(swap);
            }
            inner.append(&buttons);
            inner
        };
        root.append(&inner);
        root.update_property(&[gtk::accessible::Property::Label(&i18n("Playing on the TV"))]);
        Self {
            root,
            detail,
            title,
            release: release_button,
            swap,
            here,
        }
    }

    /// Show the card for `state`, or for a switch to the next item.
    pub fn show(&self, state: Option<&State>) {
        self.root.set_visible(true);
        self.show_swap(false);
        self.show_here(
            state.is_some(),
            &i18n("Watch here too"),
            &i18n("The player plays the video too, in step with the TV, with its own volume starting silent."),
        );
        self.release.set_label(&i18n("Use the player"));
        self.release.set_tooltip_text(Some(&i18n(
            "The TV keeps playing, and the player is free for this or another video.",
        )));
        match state {
            Some(state) => {
                self.title.set_label(&i18n("Playing on the TV"));
                self.detail.set_label(&card_line(state));
            }
            None => self.title.set_label(&i18n("Starting on the TV…")),
        }
    }

    /// Show that the player plays the video too, following the TV.
    pub fn show_synced(&self, state: &State) {
        self.show(Some(state));
        self.title.set_label(&i18n("On the TV and here"));
        self.show_here(
            true,
            &i18n("Only on the TV"),
            &i18n("The player stops following the TV and waits."),
        );
    }

    fn show_swap(&self, visible: bool) {
        if let Some(swap) = &self.swap {
            swap.set_visible(visible);
        }
    }

    fn show_here(&self, visible: bool, label: &str, tooltip: &str) {
        if let Some(here) = &self.here {
            here.set_visible(visible);
            here.set_label(label);
            here.set_tooltip_text(Some(tooltip));
        }
    }

    /// Show that the TV plays on while the player does something else;
    /// `other` when the player holds something else, which it can send.
    pub fn show_released(&self, state: &State, other: bool) {
        self.root.set_visible(true);
        self.show_swap(other);
        self.show_here(false, "", "");
        self.title.set_label(&i18n("The TV is still playing"));
        self.detail.set_label(&card_line(state));
        self.release.set_label(&i18n("Control the TV"));
        self.release.set_tooltip_text(Some(&i18n(
            "The controls drive the TV again, and the player pauses.",
        )));
    }

    pub fn hide(&self) {
        self.root.set_visible(false);
    }
}
