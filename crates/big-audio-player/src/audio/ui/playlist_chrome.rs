//! Queue-panel chrome: toolbar + empty states.
//!
//! Pure-ish view builders for the `PlaylistPanel`. Each emits an `Output` intent
//! (save-as, clear, open-files). Split out of `playlist_panel.rs` to keep that
//! file scoped to the component proper. The generic `ColumnView` chrome (column-
//! config header menu, column-width persist) lives in `column_builders` (shared
//! with the library detail view).

use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;
use big_relm4_components::feedback::tooltip;
use big_relm4_components::layout::dialog_page::BigDialogPage;
use big_relm4_components::list::action_nav_row::{BigActionNavRow, BigActionNavRowSpec};
use gtk::glib;
use relm4::ComponentSender;

use crate::i18n::i18n;

use super::playlist_panel::{PlaylistPanel, PlaylistPanelOutput};

/// Audio extensions offered by the first-run "Open Files" picker.
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "m4a", "wav", "aac", "wma", "aif", "aiff", "ape", "wv", "mka",
];

/// Queue toolbar: a left-aligned header label (playlist name when empty,
/// "N tracks · duration" when full) plus save-as / remove-duplicates / clear.
/// Each button emits an intent (save-as opens an entry dialog first). Saved-
/// playlist load, smart playlists, and import/export belong to the library
/// panel, not the queue. Returns the bar and its header label (the panel owns
/// the label so it can refresh the stats on every queue change).
pub(super) fn build_toolbar(
    sender: &ComponentSender<PlaylistPanel>,
) -> (adw::HeaderBar, gtk::Label) {
    // An AdwHeaderBar so the queue info shares the window's title row with the
    // controls (matches the god-object's content header). The sidebar header
    // owns the left window buttons; this one owns the right. Only the hamburger
    // is exposed — every queue/settings action lives in its menu, like the old
    // player (whose header buttons were hidden by default).
    let bar = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .build();
    bar.add_css_class("playlist-toolbar");

    // Centred title widget: the queue name when empty, "N tracks · duration"
    // when full (the panel refreshes it on every queue change).
    let header_label = gtk::Label::builder()
        .label(i18n("Playlist"))
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    header_label.add_css_class("heading");
    bar.set_title_widget(Some(&header_label));

    let main_menu_label = i18n("Main Menu");
    let menu_btn = tooltip::icon_button("open-menu-symbolic", &main_menu_label, &[]);
    {
        let out = sender.clone();
        menu_btn.connect_clicked(move |button| {
            let parent = button.root().and_downcast::<gtk::Window>();
            let action_source = button.clone().upcast::<gtk::Widget>().downgrade();
            show_main_menu_dialog(parent.as_ref(), action_source, &out);
        });
    }
    bar.pack_end(&menu_btn);

    (bar, header_label)
}

fn show_main_menu_dialog(
    parent: Option<&gtk::Window>,
    action_source: glib::WeakRef<gtk::Widget>,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let dialog = adw::Dialog::builder()
        .title(i18n("Main Menu"))
        .content_width(420)
        .content_height(560)
        .build();
    let page = BigDialogPage::scrolled_clamped(420, 520);
    page.content().set_spacing(12);

    let dialog_weak = dialog.downgrade();

    let library_group = menu_group(i18n("Library"));
    append_window_action_row(
        &library_group,
        &dialog_weak,
        &action_source,
        i18n("Manage Playlists"),
        "view-list-symbolic",
        "win.show-manage-playlists",
    );
    append_window_action_row(
        &library_group,
        &dialog_weak,
        &action_source,
        i18n("Music Folders"),
        "folder-music-symbolic",
        "win.add-folder",
    );
    {
        let out = sender.clone();
        append_output_row(
            &library_group,
            &dialog_weak,
            i18n("Remove Duplicates"),
            "edit-clear-symbolic",
            move || {
                let _ = out.output(PlaylistPanelOutput::RemoveDuplicates);
            },
        );
    }
    {
        let out = sender.clone();
        append_output_row(
            &library_group,
            &dialog_weak,
            i18n("Clear Queue"),
            "edit-delete-symbolic",
            move || {
                let _ = out.output(PlaylistPanelOutput::ClearQueue {
                    confirmed: true,
                    with_undo: true,
                });
            },
        );
    }
    page.content().append(&library_group);

    let sound_group = menu_group(i18n("Sound & App"));
    append_window_action_row(
        &sound_group,
        &dialog_weak,
        &action_source,
        i18n("Equalizer"),
        "audio-equalizer-symbolic",
        "win.show-equalizer",
    );
    append_window_action_row(
        &sound_group,
        &dialog_weak,
        &action_source,
        i18n("Appearance"),
        "preferences-desktop-theme-symbolic",
        "win.show-appearance",
    );
    append_window_action_row(
        &sound_group,
        &dialog_weak,
        &action_source,
        i18n("Preferences"),
        "emblem-system-symbolic",
        "win.show-preferences",
    );
    #[cfg(feature = "chromecast")]
    append_window_action_row(
        &sound_group,
        &dialog_weak,
        &action_source,
        i18n("Cast"),
        "network-wireless-symbolic",
        "win.show-cast",
    );
    page.content().append(&sound_group);

    let export_group = menu_group(i18n("Edit & Export"));
    append_window_action_row(
        &export_group,
        &dialog_weak,
        &action_source,
        i18n("Trim & Export"),
        "edit-cut-symbolic",
        "win.trim-export",
    );
    append_window_action_row(
        &export_group,
        &dialog_weak,
        &action_source,
        i18n("Convert to MP3"),
        "audio-x-generic-symbolic",
        "win.convert-mp3",
    );
    append_window_action_row(
        &export_group,
        &dialog_weak,
        &action_source,
        i18n("Convert to M4A"),
        "audio-x-generic-symbolic",
        "win.convert-m4a",
    );
    append_window_action_row(
        &export_group,
        &dialog_weak,
        &action_source,
        i18n("Convert to FLAC"),
        "audio-x-generic-symbolic",
        "win.convert-flac",
    );
    page.content().append(&export_group);

    let tools_group = menu_group(i18n("Tools"));
    append_window_action_row(
        &tools_group,
        &dialog_weak,
        &action_source,
        i18n("Sleep Timer"),
        "alarm-symbolic",
        "win.sleep-timer",
    );
    append_window_action_row(
        &tools_group,
        &dialog_weak,
        &action_source,
        i18n("Smartphone"),
        "phone-symbolic",
        "win.show-smartphone",
    );
    page.content().append(&tools_group);

    let help_group = menu_group(i18n("Help"));
    append_window_action_row(
        &help_group,
        &dialog_weak,
        &action_source,
        i18n("Keyboard Shortcuts"),
        "preferences-desktop-keyboard-shortcuts-symbolic",
        "win.show-shortcuts",
    );
    append_window_action_row(
        &help_group,
        &dialog_weak,
        &action_source,
        i18n("About"),
        "help-about-symbolic",
        "app.about",
    );
    page.content().append(&help_group);

    dialog.set_child(Some(page.root()));
    if let Some(parent) = parent {
        dialog.present(Some(parent));
    } else {
        dialog.present(None::<&gtk::Window>);
    }
}

fn menu_group(title: String) -> adw::PreferencesGroup {
    adw::PreferencesGroup::builder().title(title).build()
}

fn append_window_action_row(
    group: &adw::PreferencesGroup,
    dialog_weak: &glib::WeakRef<adw::Dialog>,
    action_source: &glib::WeakRef<gtk::Widget>,
    title: String,
    icon_name: &'static str,
    action_name: &'static str,
) {
    let dialog_weak = dialog_weak.clone();
    let action_source = action_source.clone();
    append_menu_row(group, title, icon_name, move || {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
        if let Some(source) = action_source.upgrade() {
            let _ = gtk::prelude::WidgetExt::activate_action(&source, action_name, None);
        }
    });
}

fn append_output_row<F>(
    group: &adw::PreferencesGroup,
    dialog_weak: &glib::WeakRef<adw::Dialog>,
    title: String,
    icon_name: &'static str,
    emit: F,
) where
    F: Fn() + 'static,
{
    let dialog_weak = dialog_weak.clone();
    append_menu_row(group, title, icon_name, move || {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
        emit();
    });
}

fn append_menu_row<F>(
    group: &adw::PreferencesGroup,
    title: String,
    icon_name: &'static str,
    on_activate: F,
) where
    F: Fn() + 'static,
{
    let action = Rc::new(on_activate);
    let row = BigActionNavRow::new(
        BigActionNavRowSpec::new(&title)
            .prefix_icon_name(icon_name)
            .action_label(&title),
    )
    .into_root();
    row.connect_activated(move |_| action());
    group.add(&row);
}

/// Empty-queue placeholder: a drop hint when the library already has tracks,
/// otherwise the first-run welcome page.
pub(super) fn build_empty_state(
    library_has_items: bool,
    sender: &ComponentSender<PlaylistPanel>,
) -> gtk::Widget {
    if library_has_items {
        build_drop_hint().upcast()
    } else {
        build_welcome(sender).upcast()
    }
}

/// Library populated but queue empty — invite a drag-drop or library add.
fn build_drop_hint() -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name("list-add-symbolic")
        .title(i18n("Drop tracks here"))
        .description(i18n("or use \u{FF0B} to add from the library"))
        .vexpand(true)
        .build()
}

/// First run (empty library + empty queue) — welcome with an Open Files action
/// (emits `OpenFiles`) and a few orientation tips.
fn build_welcome(sender: &ComponentSender<PlaylistPanel>) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(6)
        .vexpand(true)
        .build();
    container.add_css_class("audio-welcome");

    let icon = gtk::Image::builder()
        .icon_name("emblem-music-symbolic")
        .pixel_size(64)
        .margin_bottom(8)
        .build();
    icon.add_css_class("dim-label");
    container.append(&icon);

    let title = gtk::Label::builder()
        .label(i18n("Welcome to Big Audio Player"))
        .halign(gtk::Align::Center)
        .build();
    title.add_css_class("title-2");
    container.append(&title);

    let subtitle = gtk::Label::builder()
        .label(i18n(
            "Add a folder with your music files to get started.\n\
             Supported formats: MP3, FLAC, OGG, M4A, OPUS, WAV and more.",
        ))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .halign(gtk::Align::Center)
        .max_width_chars(48)
        .margin_top(4)
        .build();
    subtitle.add_css_class("body");
    container.append(&subtitle);

    let open_btn = gtk::Button::builder()
        .halign(gtk::Align::Center)
        .margin_top(18)
        .build();
    open_btn.add_css_class("suggested-action");
    open_btn.add_css_class("pill");
    open_btn.set_child(Some(
        &adw::ButtonContent::builder()
            .icon_name("document-open-symbolic")
            .label(i18n("Open Files"))
            .build(),
    ));
    {
        let out = sender.clone();
        open_btn.connect_clicked(move |btn| {
            let parent = btn.root().and_downcast::<gtk::Window>();
            let out = out.clone();
            FilePicker::open_files(i18n("Open Audio Files"))
                .filter(&i18n("Audio Files"), AUDIO_EXTENSIONS)
                .open_multiple_optional(parent.as_ref(), move |paths| {
                    let strs: Vec<String> = paths
                        .into_iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect();
                    if !strs.is_empty() {
                        let _ = out.output(PlaylistPanelOutput::OpenFiles(strs));
                    }
                });
        });
    }
    container.append(&open_btn);

    let tips_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .spacing(4)
        .margin_top(28)
        .build();

    let tips_header = gtk::Label::builder()
        .label(i18n("Quick tips"))
        .halign(gtk::Align::Start)
        .build();
    tips_header.add_css_class("heading");
    tips_box.append(&tips_header);

    let tips = [
        (
            "media-playlist-shuffle-symbolic",
            i18n("Drag and drop files to create a playlist"),
        ),
        (
            "folder-symbolic",
            i18n("Use the Music tab on the left to browse your library"),
        ),
        (
            "audio-speakers-symbolic",
            i18n("Explore Radio and Podcasts in the sidebar tabs"),
        ),
    ];
    for (icon_name, text) in &tips {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        row.append(
            &gtk::Image::builder()
                .icon_name(*icon_name)
                .pixel_size(16)
                .build(),
        );
        let tip_label = gtk::Label::builder()
            .label(text)
            .wrap(true)
            .xalign(0.0)
            .build();
        tip_label.add_css_class("caption");
        row.append(&tip_label);
        tips_box.append(&row);
    }
    container.append(&tips_box);

    container
}
