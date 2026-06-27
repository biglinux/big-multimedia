//! Grouping/age/search controls bar above the library tree (push-model).
//!
//! Three controls, left to right: a grouping-preset `DropDown`
//! ([`LibraryGrouping::presets`]), an age-filter `DropDown` ([`AgeFilter::all`]),
//! and a debounced `SearchEntry`. Each emits an `Output` intent — the shell owns
//! the query + tree rebuild; this bar never touches the DB. The grouping model
//! ([`crate::audio::library_grouping`]) is reused as-is; the 50 ms search
//! debounce reuses [`big_relm4_components::input::debounce::BigDebouncedAction`]
//! (no hand-rolled `glib::timeout`).

use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::list::signal_factory;
use gtk::glib;
use relm4::ComponentSender;

use big_relm4_components::input::debounce::BigDebouncedAction;

use crate::audio::library_grouping::{AgeFilter, LibraryGrouping};
use crate::audio::ui::playlist_geom::dropdown_list_factory;
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelOutput};

/// 50 ms trailing-edge debounce for the search entry (matches the legacy
/// rebuild cadence; one query per typing pause, not per keystroke).
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(50);

/// The grouping dropdown plus its selection-notify signal id, so the panel can
/// set the restored preset on boot without re-emitting `GroupingChanged`.
pub(super) struct GroupingControls {
    /// The view-mode dropdown.
    pub dropdown: gtk::DropDown,
    /// Its `selected` notify signal id (blocked during a programmatic set).
    pub selection_signal_id: glib::SignalHandlerId,
}

/// Set the dropdown to the preset matching `grouping` without firing the
/// selection-notify signal (avoids a spurious rebuild on boot).
pub(super) fn select_grouping(controls: &GroupingControls, grouping: &LibraryGrouping) {
    let idx = LibraryGrouping::presets()
        .iter()
        .position(|(_, preset)| preset == grouping)
        .unwrap_or(0) as u32;
    controls
        .dropdown
        .block_signal(&controls.selection_signal_id);
    controls.dropdown.set_selected(idx);
    controls
        .dropdown
        .unblock_signal(&controls.selection_signal_id);
}

/// Build the controls bar that sits above the library tree: grouping preset +
/// age filter dropdowns and a debounced search entry. Selections/typing emit
/// `Output` intents; the shell re-runs the query and pushes results back.
/// Returns the bar plus the grouping controls (for boot-time sync).
pub(super) fn build_grouping_bar(
    sender: &ComponentSender<LibraryPanel>,
) -> (gtk::Widget, GroupingControls) {
    let bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_start(6)
        .margin_end(6)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    bar.add_css_class("library-grouping-bar");

    // Order mirrors the god-object's `music_toolbar`: grouping preset, the
    // (expanding) search entry, the add-folder button, then the age filter.
    let controls = build_grouping_dropdown(sender);
    bar.append(&controls.dropdown);
    bar.append(&build_search_entry(sender));
    bar.append(&build_add_folder_button(sender));
    bar.append(&build_age_dropdown(sender));

    (bar.upcast(), controls)
}

/// Grouping-preset dropdown → `GroupingChanged(LibraryGrouping)`. The model rows
/// are the preset labels; the callback resolves the selected index back to its
/// `LibraryGrouping` (presets are rebuilt once for the closure, kept stable).
fn build_grouping_dropdown(sender: &ComponentSender<LibraryPanel>) -> GroupingControls {
    let presets = LibraryGrouping::presets();
    let labels: Vec<&str> = presets.iter().map(|(label, _)| label.as_str()).collect();
    let dropdown = gtk::DropDown::builder()
        .model(&gtk::StringList::new(&labels))
        .build();
    // GtkDropDown does not surface its accessible name reliably, so set it in
    // code for screen readers; a tooltip mirrors it for sighted users.
    dropdown.update_property(&[gtk::accessible::Property::Label(&i18n("Library view mode"))]);
    big_relm4_components::feedback::tooltip::set(&dropdown, &i18n("Library view mode"));
    dropdown.set_list_factory(Some(&dropdown_list_factory()));

    let out = sender.clone();
    let selection_signal_id = dropdown.connect_selected_notify(move |dropdown| {
        if let Some((_, grouping)) = presets.get(dropdown.selected() as usize) {
            let _ = out.output(LibraryPanelOutput::GroupingChanged(grouping.clone()));
        }
    });
    GroupingControls {
        dropdown,
        selection_signal_id,
    }
}

/// Age-filter dropdown → `AgeFilterChanged(index)`. Emits the raw selected index
/// (resolved via [`AgeFilter::from_index`] shell-side) to keep the model order
/// authoritative.
fn build_age_dropdown(sender: &ComponentSender<LibraryPanel>) -> gtk::DropDown {
    let names: Vec<String> = AgeFilter::all().iter().map(|a| a.display_name()).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let dropdown = gtk::DropDown::builder()
        .model(&gtk::StringList::new(&name_refs))
        .build();
    dropdown.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Filter by date added",
    ))]);
    big_relm4_components::feedback::tooltip::set(&dropdown, &i18n("Filter by date added"));
    dropdown.set_list_factory(Some(&dropdown_list_factory()));

    // Compact button display (mirrors the god-object): the default "all" preset
    // (index 0) shows only a clock icon; any active filter shows its label.
    let selected_factory = gtk::SignalListItemFactory::new();
    selected_factory.connect_setup(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let stack = gtk::Stack::new();
        let icon = gtk::Image::from_icon_name("document-open-recent-symbolic");
        icon.update_property(&[gtk::accessible::Property::Label(&i18n(
            "Filter by date added",
        ))]);
        let label = gtk::Label::new(None);
        stack.add_named(&icon, Some("icon"));
        stack.add_named(&label, Some("label"));
        list_item.set_child(Some(&stack));
    });
    selected_factory.connect_bind(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let Some(stack) = signal_factory::child_as::<gtk::Stack>(list_item) else {
            return;
        };
        if list_item.position() == 0 {
            stack.set_visible_child_name("icon");
        } else if let Some(string_object) =
            signal_factory::bound_model_as::<gtk::StringObject>(list_item)
        {
            let Some(label) = stack.child_by_name("label").and_downcast::<gtk::Label>() else {
                return;
            };
            label.set_label(&string_object.string());
            stack.set_visible_child_name("label");
        }
    });
    dropdown.set_factory(Some(&selected_factory));

    let out = sender.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        let _ = out.output(LibraryPanelOutput::AgeFilterChanged(dropdown.selected()));
    });
    dropdown
}

/// Add-music-folder button (`folder-new-symbolic`) → folder picker →
/// `AddWatchFolder`. Mirrors the god-object's `browser_add_btn` in the Music
/// tab; the shell adds the folder as a watch root and scans it.
fn build_add_folder_button(sender: &ComponentSender<LibraryPanel>) -> gtk::Button {
    let add_folder_button = gtk::Button::builder()
        .icon_name("folder-new-symbolic")
        .has_frame(false)
        .valign(gtk::Align::Center)
        .build();
    add_folder_button
        .update_property(&[gtk::accessible::Property::Label(&i18n("Add music folder"))]);
    big_relm4_components::feedback::tooltip::set(&add_folder_button, &i18n("Add music folder"));

    let out = sender.clone();
    add_folder_button.connect_clicked(move |button| {
        let parent = button.root().and_downcast::<gtk::Window>();
        let out = out.clone();
        big_app_kit::file_dialogs::FilePicker::open_folder(i18n("Add Music Folder"))
            .select_folder_optional(parent.as_ref(), move |path| {
                let _ = out.output(LibraryPanelOutput::AddWatchFolder { path });
            });
    });
    add_folder_button
}

/// Debounced search entry → `SearchTextChanged(String)`. An empty query emits
/// immediately (instant clear); non-empty queries fire one intent per pause via
/// [`BigDebouncedAction`].
fn build_search_entry(sender: &ComponentSender<LibraryPanel>) -> gtk::SearchEntry {
    let entry = gtk::SearchEntry::builder()
        .placeholder_text(i18n("Filter…"))
        .hexpand(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(&i18n("Search library"))]);

    let debouncer = BigDebouncedAction::new(SEARCH_DEBOUNCE);
    let out = sender.clone();
    entry.connect_search_changed(move |entry| {
        let text = entry.text().to_string();
        if text.is_empty() {
            // Clearing the field restores the full tree without a delay.
            let _ = out.output(LibraryPanelOutput::SearchTextChanged(String::new()));
            return;
        }
        let out = out.clone();
        debouncer.run(move || {
            let _ = out.output(LibraryPanelOutput::SearchTextChanged(text));
        });
    });
    entry
}
