//! Queue-panel chrome: toolbar + empty states.
//!
//! Pure-ish view builders for the `PlaylistPanel`. Each emits an `Output` intent
//! (save-as, clear, open-files). The list's columns live in `track_list`.

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;
use big_relm4_components::input::button_content::{BigButtonContentSpec, build_button};
use big_relm4_components::layout::{
    name_prompt::{prompt_name, trimmed_name},
    toolbar_pane::{BigToolbarHeader, BigToolbarHeaderSpec},
};
use gtk::gio;
use relm4::ComponentSender;

use crate::i18n::i18n;

use super::playlist_library_dialog::save_queue_prompt_spec;
use super::playlist_panel::{PlaylistPanel, PlaylistPanelOutput};
use big_player_ui::radio_panel::RADIO_ICON;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WelcomeTipSpec {
    icon_name: &'static str,
    text: String,
}

fn should_emit_open_audio_paths(file_paths: &[String]) -> bool {
    !file_paths.is_empty()
}

fn welcome_tip_specs() -> Vec<WelcomeTipSpec> {
    vec![
        WelcomeTipSpec {
            icon_name: "list-add-symbolic",
            text: i18n("Drag and drop files to create a playlist"),
        },
        WelcomeTipSpec {
            icon_name: "emblem-music-symbolic",
            text: i18n("Use the Music tab on the left to browse your library"),
        },
        WelcomeTipSpec {
            icon_name: RADIO_ICON,
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
    split_view: &adw::OverlaySplitView,
) -> (adw::HeaderBar, gtk::Label) {
    // An AdwHeaderBar so the queue info shares the window's title row with the
    // controls. The sidebar header owns the left window buttons; this one owns
    // the right. Only the hamburger is exposed; every queue/settings action
    // lives in its menu.
    // Centered title widget: the queue name when empty, "N tracks · duration"
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
    big_relm4_components::feedback::popover_menu::set_menu_model(&menu_button, &main_menu());
    menu_button.set_primary(true);
    install_queue_menu_actions(&menu_button, sender);
    bar.pack_end(&menu_button);

    // While the sidebar is folded, this bar is the window's only title row.
    let sidebar_button = big_relm4_components::feedback::tooltip::icon_toggle_button(
        "sidebar-show-symbolic",
        &i18n("Show Sidebar"),
        false,
        &["flat"],
    );
    split_view
        .bind_property("show-sidebar", &sidebar_button, "active")
        .bidirectional()
        .sync_create()
        .build();
    split_view
        .bind_property("collapsed", &sidebar_button, "visible")
        .sync_create()
        .build();
    split_view
        .bind_property("collapsed", bar.root(), "show-start-title-buttons")
        .sync_create()
        .build();
    bar.pack_start(&sidebar_button);

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
    let spec = save_queue_prompt_spec();
    let out = sender.clone();
    prompt_name(
        parent.map(AsRef::as_ref),
        &spec,
        trimmed_name,
        move |name| {
            let _ = out.output(PlaylistPanelOutput::SaveQueueAsPlaylist { name });
            true
        },
    );
}

/// Empty-queue placeholder: where podcast episodes come from, a drop hint when
/// the library already has tracks, otherwise the first-run welcome page.
pub(super) fn build_empty_state(
    podcasts: bool,
    library_has_items: bool,
    sender: &ComponentSender<PlaylistPanel>,
) -> gtk::Widget {
    if podcasts {
        build_podcast_hint().upcast()
    } else if library_has_items {
        build_drop_hint().upcast()
    } else {
        build_welcome(sender).upcast()
    }
}

/// Podcast queue empty: episodes come from choosing a podcast.
fn build_podcast_hint() -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name("microphone-sensitivity-high-symbolic")
        .title(i18n("No episodes queued"))
        .description(i18n("Choose a podcast on the left to queue its episodes."))
        .vexpand(true)
        .build()
}

/// Library populated but queue empty: the ways a library track gets here.
fn build_drop_hint() -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name("list-add-symbolic")
        .title(i18n("Drop tracks here"))
        .description(i18n(
            "Or double-click a track in the library, or select it and press Return.",
        ))
        .vexpand(true)
        .build()
}

/// The card over the list while a radio station holds the player.
pub(super) struct RadioCard {
    pub(super) root: gtk::Box,
    pub(super) station: gtk::Label,
    /// Hidden while the list is empty: there is nothing to play.
    pub(super) play_list: gtk::Button,
}

/// Says the radio, not the list, is playing, and offers to play the list.
pub(super) fn build_radio_card(sender: &ComponentSender<PlaylistPanel>) -> RadioCard {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Start)
        .spacing(6)
        .margin_top(48)
        .margin_start(24)
        .margin_end(24)
        .visible(false)
        .accessible_role(gtk::AccessibleRole::Group)
        .css_classes(["card", "radio-now-playing"])
        .build();
    let icon = gtk::Image::builder()
        .icon_name(RADIO_ICON)
        .pixel_size(48)
        .margin_bottom(6)
        .build();
    root.append(&icon);
    let listening = gtk::Label::new(Some(&i18n("Listening to the radio")));
    listening.add_css_class("caption-heading");
    listening.add_css_class("dim-label");
    root.append(&listening);
    let station = gtk::Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .max_width_chars(32)
        .build();
    station.add_css_class("title-2");
    root.append(&station);
    let note = gtk::Label::new(Some(&i18n("The list is not playing.")));
    note.add_css_class("dim-label");
    root.append(&note);
    let play_list = build_button(
        &BigButtonContentSpec::new("media-playback-start-symbolic", i18n("Play the list"))
            .css_classes(["suggested-action", "pill"]),
    );
    play_list.set_halign(gtk::Align::Center);
    play_list.set_margin_top(12);
    {
        let out = sender.clone();
        play_list.connect_clicked(move |_| {
            let _ = out.output(PlaylistPanelOutput::PlayQueue);
        });
    }
    root.append(&play_list);
    big_gtk_kit::accessible::relate(
        &root,
        gtk::AccessibleRelation::LabelledBy,
        &[listening.upcast_ref(), station.upcast_ref()],
    );
    RadioCard {
        root,
        station,
        play_list,
    }
}

/// First run (empty library + empty queue): welcome with an Open Files action
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
            "Open music files to play them now, or add your music folder in the Music tab.\n\
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
    fn main_menu_entries_use_actions() {
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
    fn empty_picker_emits_nothing() {
        assert!(!should_emit_open_audio_paths(&[]));
        assert!(should_emit_open_audio_paths(&[
            "/music/song.flac".to_string()
        ]));
    }

    #[test]
    fn welcome_tips_keep_order() {
        let tips = welcome_tip_specs();

        assert_eq!(
            tips.iter()
                .map(|tip| (tip.icon_name, tip.text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (
                    "list-add-symbolic",
                    "Drag and drop files to create a playlist",
                ),
                (
                    "emblem-music-symbolic",
                    "Use the Music tab on the left to browse your library",
                ),
                (RADIO_ICON, "Explore Radio and Podcasts in the sidebar tabs",),
            ]
        );
    }
}
