//! Relm4 lyrics panel — header with an expand toggle plus a three-state
//! content stack (lyrics / loading / empty).
//!
//! Push-model: the on-demand fetch (db + LRCLIB, threaded) and the paned
//! reparenting live in the shell; this component only renders the state it is
//! told to show and emits `LyricsPanelOutput::ExpandToggled` so the shell can
//! resize the lyrics/playlist split. Manual `impl Component` idiom.

use adw::prelude::*;
use gtk::pango;
use relm4::{Component, ComponentParts, ComponentSender};

use big_media_data::i18n::i18n;

/// Which page of the lyrics content stack is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsState {
    /// Rendered lyrics text.
    Lyrics,
    /// Spinner while fetching.
    Loading,
    /// "No lyrics found" placeholder.
    Empty,
}

impl LyricsState {
    /// Stack child name for this state.
    #[must_use]
    fn child_name(self) -> &'static str {
        match self {
            Self::Lyrics => "lyrics",
            Self::Loading => "loading",
            Self::Empty => "empty",
        }
    }
}

/// Display state pushed into the lyrics panel by the shell.
#[derive(Debug)]
pub enum LyricsPanelInput {
    /// Switch the visible content-stack page.
    SetState(LyricsState),
    /// Set the lyrics text and switch to the lyrics page.
    SetText(String),
}

/// Intents emitted by the lyrics panel.
#[derive(Debug)]
pub enum LyricsPanelOutput {
    /// Expand toggle flipped; `true` means "show only lyrics".
    ExpandToggled(bool),
}

/// Relm4 lyrics panel component.
pub struct LyricsPanel {
    content_stack: gtk::Stack,
    lyrics_label: gtk::Label,
}

impl Component for LyricsPanel {
    type Init = ();
    type Input = LyricsPanelInput;
    type Output = LyricsPanelOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("lyrics-panel");
        root
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        root.append(&build_header(&sender));
        root.append(&build_separator());

        let lyrics_label = build_lyrics_label();
        let lyrics_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&lyrics_label)
            .build();
        lyrics_scroll.add_css_class("lyrics-scroll");

        let content_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(200)
            .vexpand(true)
            .build();
        content_stack.add_named(&lyrics_scroll, Some("lyrics"));
        content_stack.add_named(&build_loading_box(), Some("loading"));
        content_stack.add_named(&build_empty_box(), Some("empty"));
        content_stack.set_visible_child_name("empty");
        root.append(&content_stack);

        ComponentParts {
            model: Self {
                content_stack,
                lyrics_label,
            },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            LyricsPanelInput::SetState(state) => {
                self.content_stack
                    .set_visible_child_name(state.child_name());
            }
            LyricsPanelInput::SetText(text) => {
                self.lyrics_label.set_label(&text);
                self.content_stack
                    .set_visible_child_name(LyricsState::Lyrics.child_name());
            }
        }
    }
}

/// Build the header row (icon, title, expand toggle).
fn build_header(sender: &ComponentSender<LyricsPanel>) -> gtk::Box {
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(12)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(4)
        .build();

    let header_icon = gtk::Image::builder()
        .icon_name("accessories-text-editor-symbolic")
        .pixel_size(16)
        .build();
    header_icon.add_css_class("lyrics-header-icon");
    header.append(&header_icon);

    let header_label = gtk::Label::builder()
        .label(i18n("Lyrics"))
        .hexpand(true)
        .xalign(0.0)
        .build();
    header_label.add_css_class("lyrics-header-label");
    header.append(&header_label);

    let expand_button = gtk::ToggleButton::builder()
        .icon_name("view-fullscreen-symbolic")
        .css_classes(["flat", "circular"])
        .build();
    expand_button.set_tooltip_text(Some(&i18n("Show only lyrics")));
    let s = sender.clone();
    expand_button.connect_toggled(move |button| {
        let active = button.is_active();
        if active {
            button.set_icon_name("view-restore-symbolic");
            button.set_tooltip_text(Some(&i18n("Show playlist")));
        } else {
            button.set_icon_name("view-fullscreen-symbolic");
            button.set_tooltip_text(Some(&i18n("Show only lyrics")));
        }
        let _ = s.output(LyricsPanelOutput::ExpandToggled(active));
    });
    header.append(&expand_button);

    header
}

/// Build the header/content separator.
fn build_separator() -> gtk::Separator {
    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep.add_css_class("lyrics-separator");
    sep
}

/// Build the selectable, centered lyrics text label.
fn build_lyrics_label() -> gtk::Label {
    let lyrics_label = gtk::Label::builder()
        .label("")
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Start)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .max_width_chars(60)
        .justify(gtk::Justification::Center)
        .selectable(true)
        .vexpand(true)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(16)
        .margin_end(16)
        .build();
    lyrics_label.add_css_class("lyrics-text");
    lyrics_label
}

/// Build the "searching" spinner page.
fn build_loading_box() -> gtk::Box {
    let spinner_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .spacing(12)
        .build();
    let spinner = adw::Spinner::builder()
        .width_request(32)
        .height_request(32)
        .build();
    spinner_box.append(&spinner);
    let spinner_label = gtk::Label::builder()
        .label(i18n("Searching for lyrics…"))
        .css_classes(["dim-label", "caption"])
        .build();
    spinner_box.append(&spinner_label);
    spinner_box
}

/// Build the "no lyrics found" placeholder page.
fn build_empty_box() -> gtk::Box {
    let empty_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();
    empty_box.add_css_class("lyrics-empty");

    let empty_icon = gtk::Image::builder()
        .icon_name("accessories-text-editor-symbolic")
        .pixel_size(48)
        .build();
    empty_icon.add_css_class("lyrics-empty-icon");
    empty_box.append(&empty_icon);

    let empty_label = gtk::Label::builder()
        .label(i18n("No lyrics found"))
        .css_classes(["lyrics-empty-label", "dim-label"])
        .build();
    empty_box.append(&empty_label);
    empty_box
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_maps_to_stack_child_names() {
        assert_eq!(LyricsState::Lyrics.child_name(), "lyrics");
        assert_eq!(LyricsState::Loading.child_name(), "loading");
        assert_eq!(LyricsState::Empty.child_name(), "empty");
    }
}
