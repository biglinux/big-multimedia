use super::engine::build_mpv_backend;
use super::input::set_shell_input;
use super::mpris::build_mpris_service;
use super::playback::transport_labels;
use super::*;
use big_media_components::surface::{MediaNotice, create_media_root};
use big_relm4_components::input::button_content::{BigButtonContentSpec, build_button_content};
use big_relm4_components::layout::toolbar_pane::{
    BigToolbarHeader, BigToolbarHeaderSpec, BigToolbarPane, BigToolbarPaneSpec,
};

const PLAYBACK_SHORTCUT_ACTIONS: &[&str] = &[
    "play-pause",
    "seek-forward",
    "seek-backward",
    "seek-forward-long",
    "seek-backward-long",
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
    // Root is the mountable CONTENT (a `gtk::Box`), NOT a window: the window
    // chrome (size/title/present) is supplied by the mount point — the standalone
    // boot, a `big-host` module, or a `PaneContent` mount.
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        create_media_root()
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
            if v > 0.0 { v } else { 100.0 }
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
                voice_settings: crate::shared::audio_effects::nr_settings_panel(&settings, {
                    let input = sender.input_sender().clone();
                    move || input.emit(AudioShellInput::NoiseSettingsChanged)
                })
                .upcast(),
            })
            .forward(sender.input_sender(), AudioShellInput::Playback);
        // The queue is always visible (right pane) — the transport's playlist
        // toggle is redundant, so hide it.
        playback_bar.emit(AudioPlaybackBarInput::SetPlaylistButtonVisible(false));
        playback_bar.emit(AudioPlaybackBarInput::SetPaused(true));
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
        let downloads = DownloadQueuePanel::builder().launch(()).detach();
        let podcast = PodcastPanel::builder()
            .launch(())
            .forward(sender.input_sender(), AudioShellInput::Podcast);

        // The Queue tab is a vertical split: lyrics (start, hidden until toggled)
        // over the playlist (end).
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
        // own header.
        let view_stack = gtk::Stack::new();
        view_stack
            .add_titled(library.widget(), Some("music"), &i18n("Music"))
            .set_icon_name("folder-music-symbolic");
        view_stack
            .add_titled(files.widget(), Some("files"), &i18n("Files"))
            .set_icon_name("folder-symbolic");
        view_stack
            .add_titled(radio.widget(), Some("radio"), &i18n("Radio"))
            .set_icon_name("icon_radio");
        view_stack
            .add_titled(podcast.widget(), Some("podcast"), &i18n("Podcasts"))
            .set_icon_name("microphone-sensitivity-high-symbolic");

        // Tab switches drive the podcast-mode stash/restore.
        let tab_sender = sender.input_sender().clone();
        view_stack.connect_visible_child_name_notify(move |stack| {
            if let Some(name) = stack.visible_child_name() {
                let _ = tab_sender.send(AudioShellInput::ViewTabChanged(name.to_string()));
            }
        });

        // Custom tab bar (4 toggle buttons, icon + full label). An `adw::ViewSwitcher` ellipsizes the labels
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
            let content = build_button_content(&BigButtonContentSpec::new(icon, &label));
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
        let sidebar_header = BigToolbarHeader::with_title_widget(
            BigToolbarHeaderSpec::new().show_end_title_buttons(false),
            &tab_bar,
        );
        let sidebar_pane = BigToolbarPane::new(
            BigToolbarPaneSpec::new()
                .css_class("sidebar-pane")
                .width_request(360),
        );
        sidebar_pane.add_top_bar(sidebar_header.root());
        sidebar_pane.set_content(&view_stack);

        // Now-playing card at the sidebar bottom (art + title/artist/album),
        // hidden until something plays.
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
        // window controls in one row, avoiding an empty bar.
        // The download revealer lives inside the content (just below the lyrics/
        // queue split, above the transport) rather than as a second bottom bar:
        // a collapsed `Revealer` bottom-bar still reserves ~3px, which left an
        // uncovered strip under the playback bar's album-art background. Inside
        // the content box it contributes 0 px when collapsed, so the transport
        // (the sole bottom bar) sits flush to the window edge.
        let content_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content_box.append(&lyrics_paned);
        content_box.append(downloads.widget());
        let queue_save_banner = adw::Banner::builder()
            .title(i18n("The playback queue could not be saved or restored. Your previous saved queue was kept."))
            .button_label(i18n("Retry saving"))
            .build();
        content_box.append(&queue_save_banner);
        // Messages take a row above the transport instead of covering it.
        let notice = MediaNotice::new();
        content_box.append(notice.widget());
        let retry_input = sender.input_sender().clone();
        queue_save_banner.connect_button_clicked(move |_| {
            let _ = retry_input.send(AudioShellInput::RetryQueueSave);
        });
        let content_pane = BigToolbarPane::new(BigToolbarPaneSpec::new());
        content_pane.set_content(&content_box);
        // Connected on first use (the TV dialog), never merely by starting.
        let tv = crate::shared::tv_cast::TvLink::new();
        tv.subscribe({
            let input = sender.input_sender().clone();
            move || {
                let _ = input.send(AudioShellInput::TvChanged);
            }
        });
        let tv_card = crate::shared::tv_cast::TvCard::new("tv-strip", true, true, false, {
            let input = sender.input_sender().clone();
            move |action| {
                let _ = input.send(AudioShellInput::Tv(action));
            }
        });
        content_pane.add_bottom_bar(&tv_card.root);
        content_pane.add_bottom_bar(playback_bar.widget());

        let main_paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .resize_start_child(false)
            .resize_end_child(true)
            .position(400)
            .build();
        main_paned.set_start_child(Some(sidebar_pane.root()));
        main_paned.set_end_child(Some(content_pane.root()));

        root.append(&main_paned);

        // App stylesheet (lyrics/radio/queue CSS classes).
        let provider = gtk::CssProvider::new();
        provider.load_from_resource(&format!(
            "{}/style.css",
            crate::config::audio::RESOURCE_PREFIX
        ));
        big_gtk_kit::css::add_provider_for_widget_lifetime(
            &root,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

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
        // Lists consume Left/Right for their own navigation before a bubbling
        // window shortcut sees them, so the seek keys are taken on the way
        // down. Sliders, text and the library tree keep their arrows.
        let seek_actions: std::collections::HashMap<String, String> = single_key_actions
            .iter()
            .filter(|(_, action)| action.starts_with("seek-"))
            .map(|(accel, action)| (accel.clone(), action.clone()))
            .collect();
        let seek_controller = gtk::EventControllerKey::new();
        seek_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let seek_sender = sender.input_sender().clone();
        seek_controller.connect_key_pressed(move |controller, keyval, _, state| {
            let accel = crate::audio::keybindings::key_to_accel_string(keyval, state);
            let Some(action) = seek_actions.get(accel.as_str()) else {
                return glib::Propagation::Proceed;
            };
            if focus_keeps_arrows(controller) || !dispatch_shortcut_action(action, &seek_sender) {
                return glib::Propagation::Proceed;
            }
            glib::Propagation::Stop
        });
        root.add_controller(seek_controller);
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

        // Appearance: apply the saved palette + high-contrast now, and let the dialog re-apply live via the same two
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
            let theme_provider = theme_provider.clone();
            let hc_provider = hc_provider.clone();
            root.connect_destroy(move |root| {
                for slot in [&theme_provider, &hc_provider] {
                    if let Some(provider) = slot.borrow_mut().take() {
                        gtk::style_context_remove_provider_for_display(&root.display(), &provider);
                    }
                }
            });
        }
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
            let input = sender.input_sender().clone();
            big_app_kit::desktop::install_action(&win_actions, "show-preferences", move || {
                let _ = input.send(AudioShellInput::ShowPreferences);
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
        let cast_sender = sender.input_sender().clone();
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

        // Cutting and converting happen in the audio converter GUI; grey both
        // out when it is not installed.
        let converter = converter_handoff::is_installed(converter_handoff::AUDIO_CONVERTER);
        let trim_sender = sender.input_sender().clone();
        big_app_kit::desktop::install_action(&win_actions, "trim-export", move || {
            let _ = trim_sender.send(AudioShellInput::ShowTrimExport);
        })
        .set_enabled(converter);
        let convert_sender = sender.input_sender().clone();
        big_app_kit::desktop::install_action(&win_actions, "convert", move || {
            let _ = convert_sender.send(AudioShellInput::ConvertTrack);
        })
        .set_enabled(converter);

        // Engine: DB + mpv (built once at init). The event loop forwards
        // PlayerEvents into the shell.
        let db = db::open_database().ok();
        let db_read = db::open_read_connection().ok();
        let (queue_persistence, queue_updates) =
            queue_persistence::QueuePersistence::new(db.clone());
        let close_store = queue_persistence.clone();
        crate::settings::saving::bind_write_drain_with_unsaved(&root, move || {
            close_store.unsaved()
        });
        queue_persistence.bind_failure_banner(&queue_save_banner, queue_updates);
        queue_persistence.load(&sender);
        let mpv = build_mpv_backend(&sender, &settings);
        // Standalone shutdown can precede model destruction; save while mpv is alive.
        let shutdown_handler = gio::Application::default().and_then(|app| {
            let weak = Arc::downgrade(mpv.as_ref()?);
            let handler = app.connect_shutdown(move |_| {
                if let Some(mpv) = weak.upgrade() {
                    mpv.save_resume_position();
                }
            });
            Some((app.downgrade(), handler))
        });
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
                tv,
                tv_card,
                tv_last: None,
                tv_was_released: false,
                tv_follow: None,
                tv_catch_up: false,
                trim,
                _files: files,
                radio,
                lyrics,
                lyrics_paned,
                lyrics_visible: false,
                downloads,
                _podcast: podcast,
                notice,
                podcast_mode: false,
                saved_music_queue: None,
                saved_podcast_queue: None,
                queue: PlayQueue::new(),
                queue_persistence,
                queue_ready: false,
                pending_opens: std::collections::VecDeque::new(),
                open_generation: 0,
                history: QueueHistory::default(),
                now_playing_gen: 0,
                voice_active: std::rc::Rc::new(crate::shared::popovers::VoiceStrength::new(
                    initial_voice_active,
                )),
                np_box,
                np_art,
                np_title,
                np_artist,
                np_album,
                radio_station_name: String::new(),
                recording_path: None,
                recording_generation: 0,
                mpris,
                mpris_state,
                duration_secs: 0.0,
                trim_active: false,
                trim_ranges: Vec::new(),
                trim_play_selection: false,
                mpv,
                shutdown_handler,
                db,
                db_read,
                settings,
                sleep_timer_source: None,
                sleep_timer_remaining: 0,
                sleep_timer_action: String::new(),
                sleep_timer_volume: 100.0,
                library_grouping: initial_grouping,
                library_filter: String::new(),
                library_age: 0,
                library_generation: LibraryGeneration::default(),
                scan_slot: library_scan_engine::ScanSlot::default(),
                library_queries: library_generation::LatestMailbox::default(),
                current_file: String::new(),
                last_blur_art: None,
                is_playing: false,
                is_stream: false,
                failed_loads: 0,
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
            self.publish_mpris_snapshot();
        });
    }
}

fn dispatch_shortcut_action(action: &str, input: &relm4::Sender<AudioShellInput>) -> bool {
    let message = match action {
        "play-pause" => AudioShellInput::Playback(AudioPlaybackBarOutput::TogglePlay),
        "seek-forward" => AudioShellInput::SeekBy(5.0),
        "seek-backward" => AudioShellInput::SeekBy(-5.0),
        "seek-forward-long" => AudioShellInput::SeekBy(30.0),
        "seek-backward-long" => AudioShellInput::SeekBy(-30.0),
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

/// Whether the focused widget uses Left/Right itself: text, a slider (the seek
/// bar and volume move with the arrows) or a library tree row.
fn focus_keeps_arrows(controller: &gtk::EventControllerKey) -> bool {
    if focus_accepts_text(controller) {
        return true;
    }
    let focus = controller
        .widget()
        .and_then(|widget| widget.root())
        .and_downcast::<gtk::Window>()
        .and_then(|window| gtk::prelude::GtkWindowExt::focus(&window));
    focus.is_some_and(|focus| {
        focus.is::<gtk::Range>()
            || focus.ancestor(gtk::Range::static_type()).is_some()
            // The library tree opens and closes rows with them.
            || focus
                .downcast_ref::<big_item_view::ItemView>()
                .and_then(big_item_view::ItemView::model)
                .and_then(|model| model.item(0))
                .is_some_and(|item| item.is::<gtk::TreeListRow>())
    })
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

#[cfg(test)]
mod lifetime_tests {
    use super::*;

    #[test]
    #[ignore = "requires an isolated home and GTK session"]
    fn closing_audio_releases_the_sleep_timer() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.biglinux.AudioLifetimeTest")
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let _relm = relm4::RelmApp::<()>::from_app(app);
        let player = crate::open_in_application();
        player.controller.emit(AudioShellInput::SleepTimerStart {
            minutes: 60,
            action: "pause".into(),
        });
        let context = glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
        {
            let mut state = player.controller.state().get_mut();
            state.model.is_stream = true;
            state.model.current_file = "https://example.invalid/radio".into();
        }
        let generation = player.controller.model().recording_generation;
        player
            .controller
            .emit(AudioShellInput::Radio(RadioPanelOutput::StopRecording));
        player
            .controller
            .emit(AudioShellInput::RadioRecordingPrepared {
                generation,
                source: "https://example.invalid/radio".into(),
                path: PathBuf::from("cancelled-recording.mp3"),
                result: Ok(()),
            });
        while context.pending() {
            context.iteration(false);
        }
        assert!(
            player.controller.model().recording_path.is_none(),
            "directory preparation restarted a cancelled recording"
        );
        let source = context
            .find_source_by_id(
                player
                    .controller
                    .model()
                    .sleep_timer_source
                    .as_ref()
                    .expect("timer armed"),
            )
            .unwrap();
        let backend = player
            .controller
            .model()
            .mpv
            .clone()
            .expect("audio backend");
        player.window.destroy();
        drop(player);
        for _ in 0..100 {
            while context.pending() {
                context.iteration(false);
            }
            if source.is_destroyed() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            source.is_destroyed(),
            "closed audio component kept its countdown source"
        );
        assert_eq!(
            Arc::weak_count(&backend),
            0,
            "application retained backend shutdown handler"
        );
    }
}
