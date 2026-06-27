//! Queue-panel chrome: toolbar + empty states.
//!
//! Pure-ish view builders for the `PlaylistPanel`. Each emits an `Output` intent
//! (save-as, clear, open-files). Split out of `playlist_panel.rs` to keep that
//! file scoped to the component proper. The generic `ColumnView` chrome (column-
//! config header menu, column-width persist) lives in `column_builders` (shared
//! with the library detail view).

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    hamburger_menu::{build_hamburger_menu_button, BigHamburgerMenuSpec, BigMenuActionItem},
    preferences::BigPreferencesGroupSpec,
    toolbar_pane::{BigToolbarHeader, BigToolbarHeaderSpec},
};
use gtk::{gio, glib};
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
    // Centred title widget: the queue name when empty, "N tracks · duration"
    // when full (the panel refreshes it on every queue change).
    let header_label = gtk::Label::builder()
        .label(i18n("Playlist"))
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    header_label.add_css_class("heading");
    let bar = BigToolbarHeader::with_title_widget(
        BigToolbarHeaderSpec::new()
            .show_start_title_buttons(false)
            .css_class("playlist-toolbar"),
        &header_label,
    );

    let main_menu_label = i18n("Main Menu");
    let menu_button = build_hamburger_menu_button(
        &BigHamburgerMenuSpec::new(main_menu_label).items(main_menu_items()),
    );
    install_queue_menu_actions(&menu_button, sender);
    bar.pack_end(&menu_button);

    (bar.into_root(), header_label)
}

fn main_menu_items() -> Vec<BigMenuActionItem> {
    let mut items = vec![
        BigMenuActionItem::new(i18n("Manage Playlists"), "win.show-manage-playlists"),
        BigMenuActionItem::new(i18n("Save Queue as Playlist"), "queue.save-as-playlist"),
        BigMenuActionItem::new(i18n("Music Folders"), "win.add-folder"),
        BigMenuActionItem::new(i18n("Remove Duplicates"), "queue.remove-duplicates"),
        BigMenuActionItem::new(i18n("Clear Queue"), "queue.clear"),
        BigMenuActionItem::new(i18n("Equalizer"), "win.show-equalizer"),
        BigMenuActionItem::new(i18n("Sleep Timer"), "win.sleep-timer"),
        BigMenuActionItem::new(i18n("Trim & Export"), "win.trim-export"),
        BigMenuActionItem::new(i18n("Convert to MP3"), "win.convert-mp3"),
        BigMenuActionItem::new(i18n("Convert to M4A"), "win.convert-m4a"),
        BigMenuActionItem::new(i18n("Convert to FLAC"), "win.convert-flac"),
        BigMenuActionItem::new(i18n("Smartphone"), "win.show-smartphone"),
        BigMenuActionItem::new(i18n("Appearance"), "win.show-appearance"),
        BigMenuActionItem::new(i18n("Preferences"), "win.show-preferences"),
    ];
    #[cfg(feature = "chromecast")]
    items.push(BigMenuActionItem::new(i18n("Cast"), "win.show-cast"));
    items.extend([
        BigMenuActionItem::new(i18n("Keyboard Shortcuts"), "win.show-shortcuts"),
        BigMenuActionItem::new(i18n("About"), "app.about"),
    ]);
    items
}

fn install_queue_menu_actions(
    menu_button: &gtk::MenuButton,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let actions = gio::SimpleActionGroup::new();

    {
        let out = sender.clone();
        let menu_button = menu_button.downgrade();
        big_app_kit::desktop::install_action(&actions, "save-as-playlist", move || {
            let parent = menu_button
                .upgrade()
                .and_then(|button| button.root())
                .and_downcast::<gtk::Window>();
            prompt_queue_playlist_name(parent.as_ref(), &out);
        });
    }
    {
        let out = sender.clone();
        big_app_kit::desktop::install_action(&actions, "remove-duplicates", move || {
            let _ = out.output(PlaylistPanelOutput::RemoveDuplicates);
        });
    }
    {
        let out = sender.clone();
        big_app_kit::desktop::install_action(&actions, "clear", move || {
            let _ = out.output(PlaylistPanelOutput::ClearQueue {
                confirmed: true,
                with_undo: true,
            });
        });
    }

    menu_button.insert_action_group("queue", Some(&actions));
}

fn prompt_queue_playlist_name(
    parent: Option<&gtk::Window>,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let dialog = adw::Dialog::builder()
        .title(i18n("Save Queue as Playlist"))
        .content_width(420)
        .content_height(240)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);
    page.append_description(&i18n("Name for the new playlist"));

    let group = BigPreferencesGroupSpec::untitled().build();
    let entry = adw::EntryRow::builder()
        .title(i18n("Playlist name"))
        .show_apply_button(true)
        .build();
    group.add(&entry);
    page.content().append(&group);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Save")));
    footer.accept_button().set_sensitive(false);
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let accept_button = footer.accept_button().clone();
    entry.connect_changed(move |row| {
        accept_button.set_sensitive(!row.text().trim().is_empty());
    });

    let accept_button = footer.accept_button().clone();
    entry.connect_apply(move |_| {
        if accept_button.is_sensitive() {
            accept_button.emit_clicked();
        }
    });

    let out = sender.clone();
    let entry_for_accept = entry.clone();
    let dialog_weak = dialog.downgrade();
    footer.accept_button().connect_clicked(move |_| {
        let name = entry_for_accept.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        let _ = out.output(PlaylistPanelOutput::SaveQueueAsPlaylist { name });
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let entry_for_focus = entry.clone();
    dialog.connect_map(move |_| {
        let entry_for_idle = entry_for_focus.clone();
        glib::idle_add_local_once(move || {
            entry_for_idle.grab_focus();
            entry_for_idle.select_region(0, -1);
        });
    });
    dialog.present(parent);
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

    let open_files_button = gtk::Button::builder()
        .halign(gtk::Align::Center)
        .margin_top(18)
        .build();
    open_files_button.add_css_class("suggested-action");
    open_files_button.add_css_class("pill");
    open_files_button.update_property(&[gtk::accessible::Property::Label(&i18n("Open files"))]);
    open_files_button.set_child(Some(
        &adw::ButtonContent::builder()
            .icon_name("document-open-symbolic")
            .label(i18n("Open Files"))
            .build(),
    ));
    {
        let out = sender.clone();
        open_files_button.connect_clicked(move |button| {
            let parent = button.root().and_downcast::<gtk::Window>();
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
    container.append(&open_files_button);

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

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use big_relm4_components::layout::hamburger_menu::build_flat_menu_model;

    #[test]
    fn main_menu_uses_gmenu_actions_for_every_entry() {
        let menu = build_flat_menu_model(&main_menu_items());
        let actions = big_relm4_components::layout::hamburger_menu::menu_action_names(&menu);

        for action in [
            "win.show-manage-playlists",
            "queue.save-as-playlist",
            "win.add-folder",
            "queue.remove-duplicates",
            "queue.clear",
            "win.show-equalizer",
            "win.show-appearance",
            "win.show-preferences",
            "win.trim-export",
            "win.convert-mp3",
            "win.convert-m4a",
            "win.convert-flac",
            "win.sleep-timer",
            "win.show-smartphone",
            "win.show-shortcuts",
            "app.about",
        ] {
            assert!(
                actions.iter().any(|actual| actual == action),
                "missing menu action {action}; actions={actions:?}"
            );
        }

        #[cfg(feature = "chromecast")]
        assert!(
            actions.iter().any(|actual| actual == "win.show-cast"),
            "missing chromecast menu action; actions={actions:?}"
        );
    }

    #[test]
    fn main_menu_is_flat_without_sections_or_submenus() {
        let menu = build_flat_menu_model(&main_menu_items());
        for index in 0..menu.n_items() {
            assert!(
                menu.item_link(index, "section").is_none(),
                "hamburger menu must not expose categories"
            );
            assert!(
                menu.item_link(index, "submenu").is_none(),
                "hamburger menu must not expose category submenus"
            );
        }
    }
}
