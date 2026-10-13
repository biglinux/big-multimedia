//! Queue-panel chrome: toolbar + empty states.
//!
//! Pure-ish view builders for the `PlaylistPanel`. Each emits an `Output` intent
//! (save-as, clear, open-files). The list's columns live in `track_list`.

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;
use big_relm4_components::input::button_content::{BigButtonContentSpec, build_button};
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
    toolbar_pane::{BigToolbarHeader, BigToolbarHeaderSpec},
};
use gtk::{gio, glib};
use relm4::ComponentSender;

use crate::i18n::i18n;

use super::playlist_panel::{PlaylistPanel, PlaylistPanelOutput};

#[derive(Debug, Clone, PartialEq, Eq)]
struct WelcomeTipSpec {
    icon_name: &'static str,
    text: String,
}

fn can_save_queue_playlist_name(candidate_name: &str) -> bool {
    !candidate_name.trim().is_empty()
}

fn normalized_queue_playlist_name(candidate_name: &str) -> Option<String> {
    let trimmed_name = candidate_name.trim();
    if trimmed_name.is_empty() {
        None
    } else {
        Some(trimmed_name.to_string())
    }
}

fn should_emit_open_audio_paths(file_paths: &[String]) -> bool {
    !file_paths.is_empty()
}

fn welcome_tip_specs() -> Vec<WelcomeTipSpec> {
    vec![
        WelcomeTipSpec {
            icon_name: "media-playlist-shuffle-symbolic",
            text: i18n("Drag and drop files to create a playlist"),
        },
        WelcomeTipSpec {
            icon_name: "folder-symbolic",
            text: i18n("Use the Music tab on the left to browse your library"),
        },
        WelcomeTipSpec {
            icon_name: "audio-speakers-symbolic",
            text: i18n("Explore Radio and Podcasts in the sidebar tabs"),
        },
    ]
}

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
    // controls. The sidebar header owns the left window buttons; this one owns
    // the right. Only the hamburger is exposed — every queue/settings action
    // lives in its menu.
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

    // A native menu: grouped, with each action's shortcut beside it.
    let menu_button = big_relm4_components::feedback::tooltip::icon_menu_button(
        "open-menu-symbolic",
        &i18n("Main Menu"),
        &["flat"],
    );
    menu_button.set_menu_model(Some(&main_menu()));
    menu_button.set_primary(true);
    install_queue_menu_actions(&menu_button, sender);
    bar.pack_end(&menu_button);

    (bar.into_root(), header_label)
}

/// The queue first, then the library, sound and tools, other screens and
/// the application. Each group is a shared section: GTK keeps every section
/// a menu shows.
fn main_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let groups: [(&'static str, Vec<(String, &str)>); 5] = [
        (
            "audio-menu-queue",
            vec![
                (i18n("Save Queue as Playlist"), "queue.save-as-playlist"),
                (i18n("Remove Duplicates"), "queue.remove-duplicates"),
                (i18n("Clear Queue"), "queue.clear"),
            ],
        ),
        (
            "audio-menu-library",
            vec![
                (i18n("Manage Playlists"), "win.show-manage-playlists"),
                (i18n("Music Folders"), "win.add-folder"),
            ],
        ),
        (
            "audio-menu-tools",
            vec![
                (i18n("Equalizer"), "win.show-equalizer"),
                (i18n("Sleep Timer"), "win.sleep-timer"),
                (i18n("Trim & Export"), "win.trim-export"),
                (i18n("Convert…"), "win.convert"),
            ],
        ),
        (
            "audio-menu-devices",
            vec![
                (i18n("Play on a TV"), "win.show-cast"),
                (i18n("Smartphone"), "win.show-smartphone"),
            ],
        ),
        (
            "audio-menu-app",
            vec![
                (i18n("Appearance"), "win.show-appearance"),
                (i18n("Preferences"), "win.show-preferences"),
                (i18n("Keyboard Shortcuts"), "win.show-shortcuts"),
                (i18n("About"), "app.about"),
            ],
        ),
    ];
    for (key, items) in groups {
        big_gtk_kit::menu::append_shared_section(&menu, None, key, || {
            let section = gio::Menu::new();
            for (label, action) in items {
                section.append(Some(&label), Some(action));
            }
            section
        });
    }
    menu
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
        accept_button.set_sensitive(can_save_queue_playlist_name(row.text().as_str()));
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
        let Some(name) = normalized_queue_playlist_name(entry_for_accept.text().as_str()) else {
            return;
        };
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

    let open_files_button = build_button(
        &BigButtonContentSpec::new("document-open-symbolic", i18n("Open Files"))
            .accessible_label(i18n("Open files"))
            .css_classes(["suggested-action", "pill"]),
    );
    open_files_button.set_halign(gtk::Align::Center);
    open_files_button.set_margin_top(18);
    {
        let out = sender.clone();
        open_files_button.connect_clicked(move |button| {
            let parent = button.root().and_downcast::<gtk::Window>();
            let out = out.clone();
            FilePicker::open_files(i18n("Open Audio Files"))
                .filter(
                    &i18n("Audio Files"),
                    big_media_components::media::files::AUDIO_EXTENSIONS,
                )
                .open_multiple_optional(parent.as_ref(), move |paths| {
                    let strs: Vec<String> = paths
                        .into_iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect();
                    if should_emit_open_audio_paths(&strs) {
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

    for tip in welcome_tip_specs() {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        row.append(
            &gtk::Image::builder()
                .icon_name(tip.icon_name)
                .pixel_size(16)
                .build(),
        );
        let tip_label = gtk::Label::builder()
            .label(&tip.text)
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

    #[test]
    fn main_menu_uses_gmenu_actions_for_every_entry() {
        let actions = big_relm4_components::layout::hamburger_menu::menu_action_names(&main_menu());

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
            "win.convert",
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

        assert!(
            actions.iter().any(|actual| actual == "win.show-cast"),
            "missing TV menu action; actions={actions:?}"
        );
    }

    #[test]
    fn queue_playlist_name_accepts_only_trimmed_non_empty_text() {
        assert!(!can_save_queue_playlist_name(""));
        assert!(!can_save_queue_playlist_name("  "));
        assert!(can_save_queue_playlist_name(" Road Trip "));

        assert_eq!(
            normalized_queue_playlist_name("  Road Trip  "),
            Some("Road Trip".to_string())
        );
        assert_eq!(normalized_queue_playlist_name("\t\n"), None);
    }

    #[test]
    fn open_audio_paths_emit_only_when_picker_returns_paths() {
        assert!(!should_emit_open_audio_paths(&[]));
        assert!(should_emit_open_audio_paths(&[
            "/music/song.flac".to_string()
        ]));
    }

    #[test]
    fn welcome_tip_specs_keep_expected_icon_order_and_text() {
        let tips = welcome_tip_specs();

        assert_eq!(
            tips.iter()
                .map(|tip| (tip.icon_name, tip.text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (
                    "media-playlist-shuffle-symbolic",
                    "Drag and drop files to create a playlist",
                ),
                (
                    "folder-symbolic",
                    "Use the Music tab on the left to browse your library",
                ),
                (
                    "audio-speakers-symbolic",
                    "Explore Radio and Podcasts in the sidebar tabs",
                ),
            ]
        );
    }
}
