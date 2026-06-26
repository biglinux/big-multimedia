use super::engine::build_mpv_backend;
use super::input::set_shell_input;
use super::mpris::build_mpris_service;
use super::playback::transport_labels;
use super::*;

const PLAYBACK_SHORTCUT_ACTIONS: &[&str] = &[
    "play-pause",
    "next-track",
    "prev-track",
    "toggle-repeat",
    "toggle-shuffle",
    "undo",
    "redo",
];

impl Component for AudioShell {
    type Init = ();
    type Input = AudioShellInput;
    type Output = ();
    type CommandOutput = ();
    // Root is the mountable CONTENT (a ToastOverlay), NOT a window: the window
    // chrome (size/title/present) is supplied by the mount point — the standalone
    // boot, a `big-host` module, or a `PaneContent` mount (RESTRUCTURE ADR D14).
    type Root = adw::ToastOverlay;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::ToastOverlay::new()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Window-scoped actions go in a "win" group inserted on the Root, so they
        // travel with the content into whatever window hosts it (instead of being
        // bound to a specific `ApplicationWindow`).
        let win_actions = gio::SimpleActionGroup::new();

        let settings = Settings::for_audio();
        settings.migrate_audio_appearance();
        // Restore the saved library grouping (default: Songs = presets[0], the
        // flat track list) so the tree matches the dropdown at boot.
        let initial_grouping = {
            let saved = settings.get_string(keys::LIBRARY_GROUPING);
            if saved.is_empty() {
                LibraryGrouping::songs()
            } else {
                LibraryGrouping::deserialize(&saved)
            }
        };
        let initial_noise = settings.get_double(keys::NOISE_STRENGTH).max(0.0);
        let initial_voice_active = initial_noise > 0.0;
        let initial_volume = {
            let v = f64::from(settings.get_int(keys::AUDIO_VOLUME));
            if v > 0.0 {
                v
            } else {
                100.0
            }
        };

        // Child Controllers: each panel's Output → an AudioShellInput variant.
        let library = LibraryPanel::builder()
            .launch(())
            .forward(sender.input_sender(), AudioShellInput::Library);
        let playlist_library_dialog = PlaylistLibraryDialog::builder().launch(()).forward(
            sender.input_sender(),
            AudioShellInput::PlaylistLibraryDialog,
        );
        let playlist = PlaylistPanel::builder()
            .launch(())
            .forward(sender.input_sender(), AudioShellInput::Playlist);
        let trim = AudioTrimBar::builder()
            .launch(BigAudioTrimBarLabels::translated(i18n))
            .forward(sender.input_sender(), AudioShellInput::Trim);
        let playback_bar = AudioPlaybackBar::builder()
            .launch(AudioPlaybackBarInit {
                spec: BigAudioTransportSpec::new(transport_labels()),
                max_volume: 150.0,
                initial_volume,
                initial_voice_pct: (initial_noise * 100.0).clamp(0.0, 100.0),
                trim_content: Some(trim.widget().clone().upcast()),
            })
            .forward(sender.input_sender(), AudioShellInput::Playback);
        // The queue is always visible (right pane) — the transport's playlist
        // toggle is redundant, so hide it.
        playback_bar.emit(AudioPlaybackBarInput::SetPlaylistButtonVisible(false));
        let files = FileBrowser::builder()
            .launch(FileBrowserConfig {
                is_media_file: big_media_components::media::files::is_audio_file,
                settings: crate::settings::Settings::for_audio(),
                fallback_dir: gtk::glib::user_special_dir(gtk::glib::UserDirectory::Music)
                    .unwrap_or_else(|| gtk::glib::home_dir().join("Music")),
                add_current_label: i18n("Add current folder to Music Library"),
                add_selected_label: i18n("Add this folder to Music Library"),
                filter_placeholder: i18n("Filter files…"),
            })
            .forward(sender.input_sender(), AudioShellInput::Files);
        let radio = RadioPanel::builder()
            .launch(())
            .forward(sender.input_sender(), AudioShellInput::Radio);
        let lyrics = LyricsPanel::builder()
            .launch(())
            .forward(sender.input_sender(), AudioShellInput::Lyrics);
        // Self-contained: no Output, fed only an Enqueue by the shell.
        let downloads = DownloadManagerPanel::builder().launch(()).detach();
        let podcast = PodcastPanel::builder()
            .launch(())
            .forward(sender.input_sender(), AudioShellInput::Podcast);

        // The Queue tab is a vertical split: lyrics (start, hidden until toggled)
        // over the playlist (end). Mirrors the god-object's lyrics column.
        let lyrics_paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .shrink_start_child(true)
            .shrink_end_child(false)
            .resize_start_child(true)
            .resize_end_child(true)
            .build();
        lyrics_paned.set_start_child(Some(lyrics.widget()));
        lyrics_paned.set_end_child(Some(playlist.widget()));
        lyrics.widget().set_visible(false);

        // LEFT sidebar pane (distinct `.sidebar-pane` colour, always visible):
        // Music | Files | Radio | Podcasts in a view stack switched from its
        // own header. Mirrors the god-object's split layout.
        let view_stack = adw::ViewStack::new();
        view_stack.add_titled_with_icon(
            library.widget(),
            Some("music"),
            &i18n("Music"),
            "folder-music-symbolic",
        );
        view_stack.add_titled_with_icon(
            files.widget(),
            Some("files"),
            &i18n("Files"),
            "folder-symbolic",
        );
        view_stack.add_titled_with_icon(
            radio.widget(),
            Some("radio"),
            &i18n("Radio"),
            "icon_radio",
        );
        view_stack.add_titled_with_icon(
            podcast.widget(),
            Some("podcast"),
            &i18n("Podcasts"),
            "microphone-sensitivity-high-symbolic",
        );

        // Tab switches drive the podcast-mode stash/restore.
        let tab_sender = sender.input_sender().clone();
        view_stack.connect_visible_child_name_notify(move |stack| {
            if let Some(name) = stack.visible_child_name() {
                let _ = tab_sender.send(AudioShellInput::ViewTabChanged(name.to_string()));
            }
        });

        // Custom tab bar (4 toggle buttons, icon + full label) — mirrors the
        // god-object's `.tab-bar`. An `adw::ViewSwitcher` ellipsizes the labels
        // ("Músi…", "Arqui…") at the sidebar's width; the toggle row never does.
        let tab_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .homogeneous(true)
            .hexpand(true)
            .css_classes(["tab-bar"])
            .build();
        let tabs: [(&str, String, &str); 4] = [
            ("emblem-music-symbolic", i18n("Music"), "music"),
            ("folder-symbolic", i18n("Files"), "files"),
            ("icon_radio", i18n("Radio"), "radio"),
            (
                "microphone-sensitivity-high-symbolic",
                i18n("Podcast"),
                "podcast",
            ),
        ];
        let mut tab_group: Option<gtk::ToggleButton> = None;
        for (icon, label, name) in tabs {
            let content = adw::ButtonContent::builder()
                .icon_name(icon)
                .label(&label)
                .build();
            let tab_button = gtk::ToggleButton::builder()
                .child(&content)
                .css_classes(["tab-btn", "flat"])
                .build();
            tab_button.update_property(&[gtk::accessible::Property::Label(&label)]);
            if let Some(ref g) = tab_group {
                tab_button.set_group(Some(g));
            } else {
                tab_button.set_active(true);
                tab_group = Some(tab_button.clone());
            }
            // Weak: the tab button lives in the same pane as the stack — a
            // strong capture pinned the whole sidebar stack after close.
            let stack = view_stack.downgrade();
            let target = name.to_string();
            tab_button.connect_toggled(move |button| {
                if !button.is_active() {
                    return;
                }
                if let Some(stack) = stack.upgrade() {
                    stack.set_visible_child_name(&target);
                }
            });
            tab_bar.append(&tab_button);
        }
        let sidebar_header = adw::HeaderBar::builder()
            .title_widget(&tab_bar)
            .show_end_title_buttons(false)
            .build();
        let sidebar_pane = adw::ToolbarView::new();
        sidebar_pane.add_top_bar(&sidebar_header);
        sidebar_pane.set_content(Some(&view_stack));
        sidebar_pane.add_css_class("sidebar-pane");
        sidebar_pane.set_size_request(360, -1);

        // Now-playing card at the sidebar bottom (art + title/artist/album),
        // hidden until something plays — mirrors the god-object's
        // `sidebar_now_playing`.
        let np_art = gtk::Image::builder()
            .pixel_size(48)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["playback-bar-art"])
            .build();
        np_art.update_property(&[gtk::accessible::Property::Label(&i18n("Album art"))]);
        let np_title = gtk::Label::builder()
            .label(i18n("Big Audio Player"))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(30)
            .css_classes(["playback-title"])
            .build();
        let np_artist = gtk::Label::builder()
            .label(i18n("Select a song to start"))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(30)
            .css_classes(["playback-artist"])
            .build();
        let np_album = gtk::Label::builder()
            .label("")
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(30)
            .visible(false)
            .css_classes(["playback-album"])
            .build();
        let np_text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .spacing(1)
            .build();
        np_text.append(&np_title);
        np_text.append(&np_artist);
        np_text.append(&np_album);
        let np_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .visible(false)
            .build();
        np_box.append(&np_art);
        np_box.append(&np_text);
        sidebar_pane.add_bottom_bar(&np_box);

        // RIGHT content pane: the queue (always visible) with the lyrics split,
        // and the transport bar pinned at the bottom. No header here — the queue
        // panel's own AdwHeaderBar toolbar carries the title/actions + the right
        // window controls (one row, like the god-object), avoiding an empty bar.
        // The download revealer lives inside the content (just below the lyrics/
        // queue split, above the transport) rather than as a second bottom bar:
        // a collapsed `Revealer` bottom-bar still reserves ~3px, which left an
        // uncovered strip under the playback bar's album-art background. Inside
        // the content box it contributes 0 px when collapsed, so the transport
        // (the sole bottom bar) sits flush to the window edge.
        let content_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content_box.append(&lyrics_paned);
        content_box.append(downloads.widget());
        let content_pane = adw::ToolbarView::new();
        content_pane.set_content(Some(&content_box));
        content_pane.add_bottom_bar(playback_bar.widget());

        let main_paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .resize_start_child(false)
            .resize_end_child(true)
            .position(400)
            .build();
        main_paned.set_start_child(Some(&sidebar_pane));
        main_paned.set_end_child(Some(&content_pane));

        root.set_child(Some(&main_paned));

        // App stylesheet (lyrics/radio/queue CSS classes) — was loaded by the
        // god-object window; the shell owns it now. Also register the bundled
        // icon path so app icons (e.g. the radio tab) resolve in any theme.
        if let Some(display) = gdk::Display::default() {
            gtk::IconTheme::for_display(&display)
                .add_resource_path(&format!("{}/icons", crate::config::audio::RESOURCE_PREFIX));
            let provider = gtk::CssProvider::new();
            provider.load_from_resource(&format!(
                "{}/style.css",
                crate::config::audio::RESOURCE_PREFIX
            ));
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        for action in PLAYBACK_SHORTCUT_ACTIONS {
            let input = sender.input_sender().clone();
            big_app_kit::desktop::install_action(&win_actions, action, move || {
                dispatch_shortcut_action(action, &input);
            });
        }
        let manage_folders_sender = sender.input_sender().clone();
        big_app_kit::desktop::install_action(&win_actions, "add-folder", move || {
            let _ = manage_folders_sender.send(AudioShellInput::ShowManageFolders);
        });

        // Window-level shortcuts. Modified bindings are GTK accelerators over
        // the actions above; bare single-key bindings stay here so text widgets
        // can keep normal typing behaviour.
        let key_controller = gtk::EventControllerKey::new();
        let key_sender = sender.input_sender().clone();
        let single_key_actions = crate::audio::keybindings::build_single_key_map(
            &settings.get_string(keys::CUSTOM_KEYBINDINGS),
        );
        key_controller.connect_key_pressed(move |controller, keyval, _, state| {
            if let Some(action) = media_key_action(keyval) {
                dispatch_shortcut_action(action, &key_sender);
                return glib::Propagation::Stop;
            }

            if state.contains(gdk::ModifierType::CONTROL_MASK) {
                let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
                let message = match keyval {
                    gdk::Key::z | gdk::Key::Z if shift => AudioShellInput::Redo,
                    gdk::Key::z | gdk::Key::Z => AudioShellInput::Undo,
                    gdk::Key::y | gdk::Key::Y => AudioShellInput::Redo,
                    _ => return glib::Propagation::Proceed,
                };
                let _ = key_sender.send(message);
                return glib::Propagation::Stop;
            }

            if focus_accepts_text(controller) {
                return glib::Propagation::Proceed;
            }

            let accel = crate::audio::keybindings::key_to_accel_string(keyval, state);
            let Some(action) = single_key_actions.get(accel.as_str()) else {
                return glib::Propagation::Proceed;
            };
            if dispatch_shortcut_action(action, &key_sender) {
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        root.add_controller(key_controller);

        // Main-menu window actions (the hamburger menu's GMenu references these).
        let eq_sender = sender.input_sender().clone();
        big_app_kit::desktop::install_action(&win_actions, "show-equalizer", move || {
            let _ = eq_sender.send(AudioShellInput::ShowEqualizer);
        });

        // Appearance: apply the saved palette + high-contrast now (the god-object
        // did this on boot), and let the dialog re-apply live via the same two
        // provider slots (replaced in place, never stacked).
        let theme_provider: std::rc::Rc<std::cell::RefCell<Option<gtk::CssProvider>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let hc_provider: std::rc::Rc<std::cell::RefCell<Option<gtk::CssProvider>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        crate::audio::ui::theming::apply_appearance(
            &settings,
            &mut theme_provider.borrow_mut(),
            &mut hc_provider.borrow_mut(),
        );
        {
            // Weak: these actions live in the WINDOW's action map — a strong
            // window capture is a window⇄action cycle (3 of them pinned the
            // whole window after close; standalone never noticed because
            // close was process exit).
            let root_weak = root.downgrade();
            let s = settings.clone();
            let tp = theme_provider.clone();
            let hp = hc_provider.clone();
            let blur_sender = sender.input_sender().clone();
            big_app_kit::desktop::install_action(&win_actions, "show-appearance", move || {
                let s_apply = s.clone();
                let tp = tp.clone();
                let hp = hp.clone();
                let blur_sender = blur_sender.clone();
                let Some(win) = root_weak
                    .upgrade()
                    .and_then(|content| content.root())
                    .and_downcast::<gtk::Window>()
                else {
                    return;
                };
                crate::audio::appearance::show_audio_appearance_dialog(&win, &s, move || {
                    crate::audio::ui::theming::apply_appearance(
                        &s_apply,
                        &mut tp.borrow_mut(),
                        &mut hp.borrow_mut(),
                    );
                    // The Effects toggle lives in this dialog; re-evaluate the bar.
                    let _ = blur_sender.send(AudioShellInput::RefreshBlurArt);
                });
            });
        }

        {
            let root_weak = root.downgrade();
            let s = settings.clone();
            big_app_kit::desktop::install_action(&win_actions, "show-preferences", move || {
                let Some(win) = root_weak
                    .upgrade()
                    .and_then(|content| content.root())
                    .and_downcast::<gtk::Window>()
                else {
                    return;
                };
                crate::audio::preferences::show_audio_preferences(&win, &s, None);
            });
        }

        {
            let root_weak = root.downgrade();
            let s = settings.clone();
            big_app_kit::desktop::install_action(&win_actions, "show-shortcuts", move || {
                let Some(win) = root_weak
                    .upgrade()
                    .and_then(|content| content.root())
                    .and_downcast::<gtk::Window>()
                else {
                    return;
                };
                crate::audio::ui::shortcuts_dialog::present_shortcuts(&win, &s);
            });
        }

        // Cast / Sleep Timer / Smartphone — actions send shell inputs (the shell
        // owns feature-gated cast state, timer state, and the current file path).
        #[cfg(feature = "chromecast")]
        let cast_sender = sender.input_sender().clone();
        #[cfg(feature = "chromecast")]
        big_app_kit::desktop::install_action(&win_actions, "show-cast", move || {
            let _ = cast_sender.send(AudioShellInput::ShowCast);
        });

        let smartphone_sender = sender.input_sender().clone();
        big_app_kit::desktop::install_action(&win_actions, "show-smartphone", move || {
            let _ = smartphone_sender.send(AudioShellInput::ShowSmartphone);
        });

        let sleep_sender = sender.input_sender().clone();
        big_app_kit::desktop::install_action(&win_actions, "sleep-timer", move || {
            let _ = sleep_sender.send(AudioShellInput::ShowSleepTimer);
        });

        let playlists_sender = sender.input_sender().clone();
        big_app_kit::desktop::install_action(&win_actions, "show-manage-playlists", move || {
            let _ = playlists_sender.send(AudioShellInput::ShowManagePlaylists);
        });

        let trim_sender = sender.input_sender().clone();
        big_app_kit::desktop::install_action(&win_actions, "trim-export", move || {
            let _ = trim_sender.send(AudioShellInput::ShowTrimExport);
        });
        for (name, codec) in [
            ("convert-mp3", "mp3"),
            ("convert-m4a", "aac"),
            ("convert-flac", "flac"),
        ] {
            let convert_sender = sender.input_sender().clone();
            big_app_kit::desktop::install_action(&win_actions, name, move || {
                let _ = convert_sender.send(AudioShellInput::ConvertTrack(codec.to_string()));
            });
        }

        // Engine: DB + mpv (built once at init). The event loop forwards
        // PlayerEvents into the shell.
        let db = db::open_database().ok();
        let db_read = db::open_read_connection().ok();
        #[cfg(feature = "chromecast")]
        let cast_device_registry = Arc::new(crate::cast::chromecast::CastManager::new());
        let mpv = build_mpv_backend(&sender, &settings);
        let mpris_state = Arc::new(SharedMprisState::default());
        let mpris = build_mpris_service(&sender, &mpris_state, &settings);

        // Publish the shell input so the application `open` handler can deliver
        // files opened from the desktop / CLI.
        set_shell_input(sender.input_sender().clone());

        // Sync the grouping dropdown to the restored preset (no re-emit), then
        // build the tree with that grouping + load the playlists sidebar.
        library.emit(LibraryPanelInput::SetGrouping(initial_grouping.clone()));
        sender.input(AudioShellInput::RebuildLibraryTree);
        sender.input(AudioShellInput::RefreshPlaylists);

        // Publish the "win" action group on the content Root so `win.*` actions
        // (the hamburger menu + accels) resolve through the widget hierarchy in
        // whatever window mounts this content.
        root.insert_action_group("win", Some(&win_actions));

        ComponentParts {
            model: Self {
                library,
                playlist_library_dialog,
                playlist,
                playback_bar,
                trim,
                _files: files,
                radio,
                lyrics,
                lyrics_paned,
                lyrics_visible: false,
                downloads,
                _podcast: podcast,
                toast_overlay: root.clone(),
                podcast_mode: false,
                saved_music_queue: None,
                saved_podcast_queue: None,
                queue: PlayQueue::new(),
                history: QueueHistory::default(),
                now_playing_gen: 0,
                voice_active: std::cell::Cell::new(initial_voice_active),
                np_box,
                np_art,
                np_title,
                np_artist,
                np_album,
                radio_station_name: String::new(),
                recording_path: None,
                mpris,
                mpris_state,
                duration_secs: 0.0,
                trim_active: false,
                trim_ranges: Vec::new(),
                trim_play_selection: false,
                mpv,
                db,
                db_read,
                settings,
                #[cfg(feature = "chromecast")]
                cast_device_registry,
                sleep_timer_source: None,
                sleep_timer_remaining: 0,
                sleep_timer_action: String::new(),
                sleep_timer_volume: 100.0,
                library_grouping: initial_grouping,
                library_filter: String::new(),
                library_age: 0,
                current_file: String::new(),
                last_art_bytes: None,
                is_playing: false,
                is_stream: false,
                current_playlist_name: String::new(),
            },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        // Panic boundary (module contract): a panic in the shell's message
        // handling tears this module's window down cleanly (host reaps it)
        // instead of leaving an orphaned dead window; the host process and
        // every sibling module/surface keep running.
        big_app_kit::containment::contain_embedded_update(root, "player-audio", || {
            self.handle_shell_input(message, sender);
        });
    }
}

fn dispatch_shortcut_action(action: &str, input: &relm4::Sender<AudioShellInput>) -> bool {
    let message = match action {
        "play-pause" => AudioShellInput::Playback(AudioPlaybackBarOutput::TogglePlay),
        "next-track" => AudioShellInput::Playback(AudioPlaybackBarOutput::Next),
        "prev-track" => AudioShellInput::Playback(AudioPlaybackBarOutput::Previous),
        "toggle-repeat" => AudioShellInput::Playback(AudioPlaybackBarOutput::CycleRepeat),
        "toggle-shuffle" => AudioShellInput::ToggleShuffleShortcut,
        "undo" => AudioShellInput::Undo,
        "redo" => AudioShellInput::Redo,
        _ => return false,
    };
    let _ = input.send(message);
    true
}

fn media_key_action(keyval: gdk::Key) -> Option<&'static str> {
    match keyval {
        gdk::Key::AudioPlay | gdk::Key::AudioPause | gdk::Key::AudioMedia => Some("play-pause"),
        gdk::Key::AudioNext => Some("next-track"),
        gdk::Key::AudioPrev => Some("prev-track"),
        _ => None,
    }
}

fn focus_accepts_text(controller: &gtk::EventControllerKey) -> bool {
    let Some(widget) = controller.widget() else {
        return false;
    };
    let Some(window) = widget.root().and_downcast::<gtk::Window>() else {
        return false;
    };
    let Some(focus) = gtk::prelude::GtkWindowExt::focus(&window) else {
        return false;
    };
    focus.dynamic_cast_ref::<gtk::Editable>().is_some()
        || focus.is::<gtk::TextView>()
        || focus.ancestor(gtk::TextView::static_type()).is_some()
}
