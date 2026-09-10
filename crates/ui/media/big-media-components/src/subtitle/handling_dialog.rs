//! Subtitle handling dialog for media converters.

#![allow(clippy::needless_borrows_for_generic_args)]

use adw::prelude::*;
use big_relm4_components::layout::dialog_page::BigDialogPage;
use relm4::gtk;

use crate::converter::dialog_cards::{make_card, make_dropdown};

/// Translated strings for the subtitle handling dialog.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigSubtitleHandlingDialogText {
    pub title: String,
    pub description: String,
    pub handling_title: String,
    pub handling_description: String,
    pub extract_title: String,
    pub extract_description: String,
}

impl BigSubtitleHandlingDialogText {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            title: text("Subtitles"),
            description: text("Configure how subtitles are handled during conversion."),
            handling_title: text("Subtitle Handling"),
            handling_description: text(
                "Choose how subtitles are included in the output. 'Embedded' keeps them inside the video file. 'Extract to .srt' creates separate subtitle files.",
            ),
            extract_title: text("Only Extract Subtitles"),
            extract_description: text(
                "Skip video conversion entirely and only extract the subtitle tracks to .srt files.",
            ),
        }
    }
}

/// References to source widgets mirrored by the subtitle handling dialog.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy)]
pub struct BigSubtitleHandlingDialogControls<'a> {
    pub subtitle_labels: &'a [&'a str],
    pub subtitle_combo: &'a adw::ComboRow,
    pub only_extract_switch: &'a adw::SwitchRow,
}

/// Present the subtitle handling dialog as a modal child of `parent`.
pub fn show_subtitle_handling_dialog(
    parent: &impl IsA<gtk::Widget>,
    text: BigSubtitleHandlingDialogText,
    controls: BigSubtitleHandlingDialogControls<'_>,
) {
    let dialog = adw::Dialog::builder()
        .title(&text.title)
        .content_width(560)
        .content_height(380)
        .build();

    let page = BigDialogPage::plain();
    let content = page.content().clone();
    page.append_description(&text.description);

    let sub_dd = make_dropdown(controls.subtitle_labels);
    sync_dropdown_with_combo(&sub_dd, controls.subtitle_combo);
    content.append(&make_card(
        "subtitle_keep.svg",
        &text.handling_title,
        &text.handling_description,
        &sub_dd,
    ));

    let extract_switch = gtk::Switch::builder()
        .active(controls.only_extract_switch.is_active())
        .valign(gtk::Align::Center)
        .build();
    extract_switch.update_property(&[gtk::accessible::Property::Label(&text.extract_title)]);
    {
        let source = controls.only_extract_switch.clone();
        extract_switch.connect_active_notify(move |switch| {
            source.set_active(switch.is_active());
        });
    }
    content.append(&make_card(
        "subtitle_extract.svg",
        &text.extract_title,
        &text.extract_description,
        &extract_switch,
    ));

    let toolbar = page.into_root();
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

fn sync_dropdown_with_combo(dialog_dd: &gtk::DropDown, source_combo: &adw::ComboRow) {
    // Delegate to the shared weak-bind helper — a local two-way strong bind is a
    // mutual reference cycle that leaks both widgets when the dialog closes.
    big_relm4_components::input::dropdown::sync_dropdown_with_combo_row(dialog_dd, source_combo);
}
