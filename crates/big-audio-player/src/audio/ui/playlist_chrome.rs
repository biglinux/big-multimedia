//! Queue-panel chrome: toolbar + empty states.
//!
//! Pure-ish view builders for the `PlaylistPanel`. Each emits an `Output` intent
//! (save-as, clear, open-files). Split out of `playlist_panel.rs` to keep that
//! file scoped to the component proper. The generic `ColumnView` chrome (column-
//! config header menu, column-width persist) lives in `column_builders` (shared
//! with the library detail view).

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;
use gtk::gio;
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

    // Queue actions live in a local "queue" action group so the hamburger menu
    // can reference them (queue.*); the shell-owned dialogs use win.* actions.
    let queue_actions = gio::SimpleActionGroup::new();

    {
        let out = sender.clone();
        big_app_kit::desktop::install_action(&queue_actions, "dedupe", move || {
            let _ = out.output(PlaylistPanelOutput::RemoveDuplicates);
        });
    }

    {
        let out = sender.clone();
        big_app_kit::desktop::install_action(&queue_actions, "clear", move || {
            let _ = out.output(PlaylistPanelOutput::ClearQueue {
                confirmed: true,
                with_undo: true,
            });
        });
    }
    bar.insert_action_group("queue", Some(&queue_actions));

    // Primary "more options" menu (hamburger), three sections mirroring the
    // god-object: queue tools · settings · help.
    let menu = gio::Menu::new();

    let queue_section = gio::Menu::new();
    queue_section.append(
        Some(&i18n("Manage Playlists")),
        Some("win.show-manage-playlists"),
    );
    queue_section.append(Some(&i18n("Music Folders")), Some("win.add-folder"));
    queue_section.append(Some(&i18n("Remove Duplicates")), Some("queue.dedupe"));
    queue_section.append(Some(&i18n("Clear Queue")), Some("queue.clear"));
    menu.append_section(None, &queue_section);

    let settings_section = gio::Menu::new();
    settings_section.append(Some(&i18n("Equalizer")), Some("win.show-equalizer"));
    settings_section.append(Some(&i18n("Appearance")), Some("win.show-appearance"));
    settings_section.append(Some(&i18n("Preferences")), Some("win.show-preferences"));
    #[cfg(feature = "chromecast")]
    settings_section.append(Some(&i18n("Cast")), Some("win.show-cast"));
    menu.append_section(None, &settings_section);

    let export_section = gio::Menu::new();
    export_section.append(Some(&i18n("Trim & Export…")), Some("win.trim-export"));
    let convert = gio::Menu::new();
    convert.append(Some(&i18n("MP3")), Some("win.convert-mp3"));
    convert.append(Some(&i18n("M4A")), Some("win.convert-m4a"));
    convert.append(Some(&i18n("FLAC")), Some("win.convert-flac"));
    export_section.append_submenu(Some(&i18n("Convert to")), &convert);
    menu.append_section(None, &export_section);

    let tools_section = gio::Menu::new();
    tools_section.append(Some(&i18n("Sleep Timer")), Some("win.sleep-timer"));
    tools_section.append(Some(&i18n("Smartphone")), Some("win.show-smartphone"));
    menu.append_section(None, &tools_section);

    let help_section = gio::Menu::new();
    help_section.append(
        Some(&i18n("Keyboard Shortcuts")),
        Some("win.show-shortcuts"),
    );
    help_section.append(Some(&i18n("About")), Some("app.about"));
    menu.append_section(None, &help_section);

    let menu_btn = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .primary(true)
        .menu_model(&menu)
        .build();
    crate::shared::tooltip::set(&menu_btn, &i18n("Main Menu"));
    menu_btn.update_property(&[gtk::accessible::Property::Label(&i18n("Main Menu"))]);
    bar.pack_end(&menu_btn);

    (bar, header_label)
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
