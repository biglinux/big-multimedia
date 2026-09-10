//! Extra converter options dialog.

#![allow(clippy::needless_borrows_for_generic_args)]

use adw::prelude::*;
use big_relm4_components::layout::dialog_page::BigDialogPage;
use relm4::gtk;

use crate::converter::dialog_cards::make_card;

const DIALOG_CONTENT_WIDTH: i32 = 560;
const DIALOG_CONTENT_HEIGHT: i32 = 500;
const ENTRY_MARGIN_START: i32 = 16;
const ENTRY_MARGIN_END: i32 = 16;
const ENTRY_MARGIN_BOTTOM: i32 = 16;
const PROFILE_BUTTON_ROW_SPACING: i32 = 12;
const FFMPEG_FLAGS_ILLUSTRATION: &str = "ffmpeg_flags.svg";
const RENDER_MODE_ILLUSTRATION: &str = "render_mode.svg";
const PROFILE_SETTINGS_ILLUSTRATION: &str = "reset_settings.svg";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ConverterExtraDialogPlan {
    content_width: i32,
    content_height: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ConverterExtraCardSpec {
    illustration_name: &'static str,
    title: String,
    description: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ConverterExtraEntryLayout {
    margin_start: i32,
    margin_end: i32,
    margin_bottom: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConverterExtraDialogSpec {
    ffmpeg_card: ConverterExtraCardSpec,
    ffmpeg_placeholder: String,
    ffmpeg_entry_layout: ConverterExtraEntryLayout,
    render_card: ConverterExtraCardSpec,
    render_options: [String; 2],
    profile_card: ConverterExtraCardSpec,
    export_profile_label: String,
    import_profile_label: String,
    profile_button_row_spacing: i32,
}

/// Translated strings for the converter "Extra" dialog (advanced options).
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigConverterExtraDialogText {
    pub title: String,
    pub description: String,
    pub ffmpeg_title: String,
    pub ffmpeg_description: String,
    pub ffmpeg_placeholder: String,
    pub render_title: String,
    pub render_description: String,
    pub render_auto: String,
    pub render_software: String,
    pub profile_title: String,
    pub profile_description: String,
    pub export_profile: String,
    pub import_profile: String,
}

impl BigConverterExtraDialogText {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            title: text("Extra"),
            description: text(
                "Additional options for advanced users. The defaults work well for most cases.",
            ),
            ffmpeg_title: text("Additional FFmpeg Options"),
            ffmpeg_description: text(
                "Custom flags passed directly to FFmpeg. Example: '-ss 60 -t 30' to convert only 30 seconds starting at 1 minute.",
            ),
            ffmpeg_placeholder: text("e.g.: -ss 60 -t 30"),
            render_title: text("Video Preview Rendering"),
            render_description: text(
                "How the video editor preview is rendered. 'Automatic' works best for most systems. Try 'Software' if you see display issues. Requires restart.",
            ),
            render_auto: text("Automatic"),
            render_software: text("Software"),
            profile_title: text("Export / Import Profile"),
            profile_description: text(
                "Save your current conversion settings to a file or load settings from a previously exported profile.",
            ),
            export_profile: text("Export Profile"),
            import_profile: text("Import Profile"),
        }
    }
}

/// References to source widgets mirrored by the extra dialog.
#[derive(Debug, Clone, Copy)]
pub struct BigConverterExtraDialogControls<'a> {
    /// Entry containing the user's raw FFmpeg flags string.
    pub ffmpeg_options_entry: &'a gtk::Entry,
}

/// Present the converter "Extra" dialog with profile export/import callbacks.
pub fn show_converter_extra_dialog(
    parent: &impl IsA<gtk::Widget>,
    text: BigConverterExtraDialogText,
    controls: BigConverterExtraDialogControls<'_>,
    on_export_profile: impl Fn() + 'static,
    on_import_profile: impl Fn() + 'static,
) {
    let dialog_plan = converter_extra_dialog_plan();
    let dialog_spec = converter_extra_dialog_spec(&text);

    let dialog = adw::Dialog::builder()
        .title(&text.title)
        .content_width(dialog_plan.content_width)
        .content_height(dialog_plan.content_height)
        .build();

    let page = BigDialogPage::scrolled();
    let content = page.content().clone();
    page.append_description(&text.description);

    let ffmpeg_entry = controls.ffmpeg_options_entry.clone();
    ffmpeg_entry.set_hexpand(true);
    ffmpeg_entry.set_placeholder_text(Some(&dialog_spec.ffmpeg_placeholder));
    let ffmpeg_card = make_card(
        dialog_spec.ffmpeg_card.illustration_name,
        &dialog_spec.ffmpeg_card.title,
        &dialog_spec.ffmpeg_card.description,
        &gtk::Box::new(gtk::Orientation::Vertical, 0),
    );
    let entry_wrapper = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    entry_wrapper.set_margin_start(dialog_spec.ffmpeg_entry_layout.margin_start);
    entry_wrapper.set_margin_end(dialog_spec.ffmpeg_entry_layout.margin_end);
    entry_wrapper.set_margin_bottom(dialog_spec.ffmpeg_entry_layout.margin_bottom);
    entry_wrapper.append(&ffmpeg_entry);
    ffmpeg_card.append(&entry_wrapper);
    content.append(&ffmpeg_card);

    let render_dropdown = gtk::DropDown::from_strings(&[
        dialog_spec.render_options[0].as_str(),
        dialog_spec.render_options[1].as_str(),
    ]);
    content.append(&make_card(
        dialog_spec.render_card.illustration_name,
        &dialog_spec.render_card.title,
        &dialog_spec.render_card.description,
        &render_dropdown,
    ));

    let button_box = gtk::Box::new(
        gtk::Orientation::Horizontal,
        dialog_spec.profile_button_row_spacing,
    );
    button_box.set_halign(gtk::Align::Center);
    button_box.set_valign(gtk::Align::Center);

    let export_button = gtk::Button::builder()
        .label(&dialog_spec.export_profile_label)
        .css_classes(["suggested-action", "pill"])
        .build();
    let import_button = gtk::Button::builder()
        .label(&dialog_spec.import_profile_label)
        .css_classes(["pill"])
        .build();
    button_box.append(&export_button);
    button_box.append(&import_button);

    {
        // Weak: the dialog owns these buttons (set_child below), so a strong
        // clone in their click closures forms a dialog ⇄ button ref cycle that
        // leaks the whole dialog subtree on every open (close() alone never
        // breaks it).
        let dialog = dialog.downgrade();
        export_button.connect_clicked(move |_| {
            on_export_profile();
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
    }
    {
        let dialog = dialog.downgrade();
        import_button.connect_clicked(move |_| {
            on_import_profile();
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
    }

    content.append(&make_card(
        dialog_spec.profile_card.illustration_name,
        &dialog_spec.profile_card.title,
        &dialog_spec.profile_card.description,
        &button_box,
    ));

    let toolbar = page.into_root();
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

fn converter_extra_dialog_plan() -> ConverterExtraDialogPlan {
    ConverterExtraDialogPlan {
        content_width: DIALOG_CONTENT_WIDTH,
        content_height: DIALOG_CONTENT_HEIGHT,
    }
}

fn converter_extra_dialog_spec(text: &BigConverterExtraDialogText) -> ConverterExtraDialogSpec {
    ConverterExtraDialogSpec {
        ffmpeg_card: ConverterExtraCardSpec {
            illustration_name: FFMPEG_FLAGS_ILLUSTRATION,
            title: text.ffmpeg_title.clone(),
            description: text.ffmpeg_description.clone(),
        },
        ffmpeg_placeholder: text.ffmpeg_placeholder.clone(),
        ffmpeg_entry_layout: ConverterExtraEntryLayout {
            margin_start: ENTRY_MARGIN_START,
            margin_end: ENTRY_MARGIN_END,
            margin_bottom: ENTRY_MARGIN_BOTTOM,
        },
        render_card: ConverterExtraCardSpec {
            illustration_name: RENDER_MODE_ILLUSTRATION,
            title: text.render_title.clone(),
            description: text.render_description.clone(),
        },
        render_options: [text.render_auto.clone(), text.render_software.clone()],
        profile_card: ConverterExtraCardSpec {
            illustration_name: PROFILE_SETTINGS_ILLUSTRATION,
            title: text.profile_title.clone(),
            description: text.profile_description.clone(),
        },
        export_profile_label: text.export_profile.clone(),
        import_profile_label: text.import_profile.clone(),
        profile_button_row_spacing: PROFILE_BUTTON_ROW_SPACING,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BigConverterExtraDialogText, DIALOG_CONTENT_HEIGHT, DIALOG_CONTENT_WIDTH,
        ENTRY_MARGIN_BOTTOM, ENTRY_MARGIN_END, ENTRY_MARGIN_START, FFMPEG_FLAGS_ILLUSTRATION,
        PROFILE_BUTTON_ROW_SPACING, PROFILE_SETTINGS_ILLUSTRATION, RENDER_MODE_ILLUSTRATION,
        converter_extra_dialog_plan, converter_extra_dialog_spec,
    };

    #[test]
    fn translated_text_routes_every_visible_message_through_callback() {
        let mut translated_messages = Vec::new();
        let text = BigConverterExtraDialogText::translated(|message| {
            translated_messages.push(message.to_string());
            format!("translated:{message}")
        });

        assert_eq!(text.title, "translated:Extra");
        assert_eq!(
            text.description,
            "translated:Additional options for advanced users. The defaults work well for most cases."
        );
        assert_eq!(text.ffmpeg_title, "translated:Additional FFmpeg Options");
        assert_eq!(text.ffmpeg_placeholder, "translated:e.g.: -ss 60 -t 30");
        assert_eq!(text.render_title, "translated:Video Preview Rendering");
        assert_eq!(text.render_auto, "translated:Automatic");
        assert_eq!(text.render_software, "translated:Software");
        assert_eq!(text.profile_title, "translated:Export / Import Profile");
        assert_eq!(text.export_profile, "translated:Export Profile");
        assert_eq!(text.import_profile, "translated:Import Profile");
        assert_eq!(translated_messages.len(), 13);
        assert!(translated_messages.contains(&"Video Preview Rendering".to_string()));
        assert!(translated_messages.contains(&"Import Profile".to_string()));
    }

    #[test]
    fn dialog_plan_matches_extra_surface_size() {
        let plan = converter_extra_dialog_plan();

        assert_eq!(plan.content_width, DIALOG_CONTENT_WIDTH);
        assert_eq!(plan.content_height, DIALOG_CONTENT_HEIGHT);
        assert_eq!(DIALOG_CONTENT_WIDTH, 560);
        assert_eq!(DIALOG_CONTENT_HEIGHT, 500);
    }

    #[test]
    fn dialog_spec_keeps_card_illustrations_and_text() {
        let text =
            BigConverterExtraDialogText::translated(|message| format!("translated:{message}"));
        let spec = converter_extra_dialog_spec(&text);

        assert_eq!(
            spec.ffmpeg_card.illustration_name,
            FFMPEG_FLAGS_ILLUSTRATION
        );
        assert_eq!(
            spec.ffmpeg_card.title,
            "translated:Additional FFmpeg Options"
        );
        assert_eq!(
            spec.ffmpeg_card.description,
            "translated:Custom flags passed directly to FFmpeg. Example: '-ss 60 -t 30' to convert only 30 seconds starting at 1 minute."
        );
        assert_eq!(spec.render_card.illustration_name, RENDER_MODE_ILLUSTRATION);
        assert_eq!(spec.render_card.title, "translated:Video Preview Rendering");
        assert_eq!(
            spec.profile_card.illustration_name,
            PROFILE_SETTINGS_ILLUSTRATION
        );
        assert_eq!(
            spec.profile_card.title,
            "translated:Export / Import Profile"
        );
    }

    #[test]
    fn dialog_spec_keeps_ffmpeg_entry_and_render_options() {
        let text = BigConverterExtraDialogText::translated(|message| message.to_string());
        let spec = converter_extra_dialog_spec(&text);

        assert_eq!(spec.ffmpeg_placeholder, "e.g.: -ss 60 -t 30");
        assert_eq!(spec.ffmpeg_entry_layout.margin_start, ENTRY_MARGIN_START);
        assert_eq!(spec.ffmpeg_entry_layout.margin_end, ENTRY_MARGIN_END);
        assert_eq!(spec.ffmpeg_entry_layout.margin_bottom, ENTRY_MARGIN_BOTTOM);
        assert_eq!(spec.render_options, ["Automatic", "Software"]);
    }

    #[test]
    fn dialog_spec_keeps_profile_button_contract() {
        let text = BigConverterExtraDialogText::translated(|message| message.to_string());
        let spec = converter_extra_dialog_spec(&text);

        assert_eq!(spec.export_profile_label, "Export Profile");
        assert_eq!(spec.import_profile_label, "Import Profile");
        assert_eq!(spec.profile_button_row_spacing, PROFILE_BUTTON_ROW_SPACING);
        assert_eq!(PROFILE_BUTTON_ROW_SPACING, 12);
    }
}
