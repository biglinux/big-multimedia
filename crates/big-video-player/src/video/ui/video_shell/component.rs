use super::actions::{build_welcome_screen, install_window_actions, toggle_keep_above};
use super::dialogs::reapply_theme;
use super::input::{clear_shell_input, set_shell_input};
use super::input_gestures::{wire_osd_autohide, wire_osd_focus_out, wire_video_input};
use super::render::{
    FastRenderContextSlot, VideoRealizeOptions, VideoStart, wire_close, wire_realize,
};
use super::*;
use big_media_components::surface::{
    BigMediaChrome, BigMediaChromePlacement, BigMediaOverlayStack, MediaNotice, create_media_root,
    toggle_surface_fullscreened,
};
#[cfg(all(test, not(miri)))]
use big_relm4_components::layout::hamburger_menu::build_flat_menu_model;
use big_relm4_components::layout::hamburger_menu::{
    BigHamburgerMenuSpec, BigMenuActionItem, build_flat_action_popover_button,
};

impl Component for VideoShell {
    type Init = ();
    type Input = VideoShellInput;
    type Output = ();
    type CommandOutput = ();
    // Root is the mountable CONTENT (notice + split view), not a window: the window
    // chrome + window-scoped setup are supplied / resolved by the mount point
    // (standalone boot, host module, or pane).
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        create_media_root()
    }

    fn init(
        (): Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        crate::settings::bind_write_drain(&root);
        let settings = Settings::for_video();

        // App stylesheet, plus a scoped CSS class for black bg + rounded corners.
        let provider = gtk::CssProvider::new();
        provider.load_from_resource(&format!(
            "{}/style.css",
            crate::config::video::RESOURCE_PREFIX
        ));
        big_gtk_kit::css::add_provider_for_widget_lifetime(
            &root,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        root.add_css_class("big-video-player");

        // Shared mpv handle: created here so the controls-bar popovers can read
        // it at call time, and so the realize handler can fill it.
        let mpv: Rc<RefCell<Option<Arc<MpvBackend>>>> = Rc::new(RefCell::new(None));
        let video_transform = media_adjust::VideoTransformState::default();

        // Interactive trim bar, built first so its widget can be mounted inside
        // the controls bar (shown in place of the transport row in trim mode, not
        // a separate bar). Its typed outputs route back into the shell.
        let trim = trim::VideoTrimBar::builder()
            .launch(())
            .forward(sender.input_sender(), VideoShellInput::TrimBar);

        // OSD controls bar (app-local). The trim
        // bar rides inside it (transport ↔ trim stack).
        // Connected on first use (the TV page), never merely by starting.
        let tv = crate::shared::tv_cast::TvLink::new();
        tv.subscribe(|| super::send_to_shell(VideoShellInput::TvChanged));
        let controls = VideoControlsBar::builder()
            .launch(VideoControlsBarInit {
                settings: settings.clone(),
                mpv: mpv.clone(),
                tv: tv.clone(),
                trim_content: trim.widget().clone().upcast::<gtk::Widget>(),
            })
            .forward(sender.input_sender(), VideoShellInput::Controls);
        controls.emit(VideoControlsBarInput::SetHasMedia(false));

        // Folder-browser playlist sidebar (folder scan = the playlist).
        let playlist = VideoBrowserSidebar::builder()
            .launch(settings.clone())
            .forward(sender.input_sender(), VideoShellInput::Playlist);

        // IPTV browser (video-local). Held detached as a dialog child; play
        // intents route back to the shell → mpv.
        let iptv = VideoIptvBrowser::builder()
            .launch(())
            .forward(sender.input_sender(), VideoShellInput::Iptv);

        // Video surface: a black bg box is the overlay's main child; the GLArea
        // is an overlay child (the offload path hides it and stacks a
        // GraphicsOffload widget). The transport rides as a bottom overlay.
        let video_bg = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .build();
        video_bg.add_css_class("background");

        let gl_area = gtk::GLArea::new();

        let media_stack = BigMediaOverlayStack::new(&video_bg);
        let main_overlay = media_stack.overlay().clone();
        media_stack.add_primary_media_surface(&gl_area);

        // Software (no-GL) video surface: hidden until the GL render context
        // fails to come up (headless software compositor, GL-less VM). The SW
        // render path paints mpv frames here as memory textures (cairo path).
        let sw_picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .hexpand(true)
            .vexpand(true)
            .visible(false)
            .build();
        media_stack.add_fallback_media_surface(&sw_picture);

        // The header's main button opens the settings panel, not a menu.
        let settings_button = gtk::Button::builder()
            .icon_name("open-menu-symbolic")
            .action_name("win.settings")
            .tooltip_text(i18n("Settings"))
            .css_classes(["osd-btn"])
            .build();
        settings_button.update_property(&[gtk::accessible::Property::Label(&i18n("Settings"))]);
        let chrome = BigMediaChrome::new(&settings_button, true);
        // The OSD header floats over edge-to-edge video with the compositor's
        // native title buttons disabled, so the standalone window has to draw
        // its own minimize/maximize/close.
        chrome.show_end_window_controls(&i18n("Minimize"), &i18n("Maximize"), &i18n("Close"));

        // Beside the settings button, small pills say what is away from
        // normal and offer HDR for a grey picture, without covering video.
        let adjustments_pill = gtk::Button::builder()
            .action_name("win.settings")
            .css_classes(["header-pill"])
            .visible(false)
            .build();
        let hdr_hint = gtk::Button::builder()
            .label(i18n("Use HDR?"))
            .tooltip_text(i18n(
                "Does the picture look grey and washed out? Click to use HDR colours",
            ))
            .css_classes(["header-pill", "hdr-hint"])
            .visible(false)
            .build();
        hdr_hint.connect_clicked({
            let sender = sender.clone();
            move |hint| {
                hint.set_visible(false);
                sender.input(VideoShellInput::RestoreAdjustments(vec![
                    super::adjustments::Adjustment::SourceColours(1),
                ]));
            }
        });
        let tv_menu = crate::shared::tv_cast::TvCard::new("tv-menu", false, true, true, {
            let sender = sender.clone();
            move |action| sender.input(VideoShellInput::Tv(action))
        });
        let tv_pill = gtk::MenuButton::builder()
            .label(i18n("On the TV"))
            .tooltip_text(i18n("The TV is still playing"))
            .css_classes(["header-pill"])
            .popover(&gtk::Popover::builder().child(&tv_menu.root).build())
            .visible(false)
            .build();
        chrome.header().pack_end(&adjustments_pill);
        chrome.header().pack_end(&hdr_hint);
        chrome.header().pack_end(&tv_pill);
        chrome.set_bottom_child(controls.widget());
        chrome.bottom_revealer().connect_reveal_child_notify({
            let sender = sender.clone();
            move |revealer| {
                if revealer.reveals_child() {
                    sender.input(VideoShellInput::RefreshAdjustments);
                }
            }
        });
        // Dialogs change these settings; closing one brings the chips up to date.
        root.connect_realize({
            let sender = sender.clone();
            move |root| {
                if let Some(window) = root.root().and_downcast::<adw::ApplicationWindow>() {
                    let sender = sender.clone();
                    window.connect_visible_dialog_notify(move |window| {
                        if window.visible_dialog().is_none() {
                            sender.input(VideoShellInput::RefreshAdjustments);
                        }
                    });
                }
            }
        });
        media_stack.install_chrome(&chrome, BigMediaChromePlacement::Bottom);

        // Welcome screen: shown until the first file loads (hidden on
        // `FileLoaded`). A launch that opens files starts without it, on the
        // plain video area; it appears only if nothing could be opened.
        let opening_files = super::input::take_opening_files();
        let welcome = build_welcome_screen(&sender);
        welcome.set_visible(!opening_files);
        media_stack.add_welcome_overlay(&welcome);

        // Buffering spinner: centered overlay, hidden until `Buffering(true)`.
        let buffering_overlay = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        buffering_overlay.append(
            &adw::Spinner::builder()
                .width_request(48)
                .height_request(48)
                .build(),
        );
        media_stack.add_status_overlay(&buffering_overlay);

        // Drag-and-drop a file onto the window → open it (old `setup_drag_drop`).
        {
            let sender = sender.clone();
            big_app_kit::widgets::add_file_drop_target(&main_overlay, move |paths| {
                let Some(path) = paths.first().and_then(|p| p.to_str()) else {
                    return false;
                };
                sender.input(VideoShellInput::OpenFiles(vec![path.to_string()]));
                true
            });
        }

        // The playlist / folder browser floats over the video on the left.
        let playlist_panel = super::playlist_panel::PlaylistPanel::new(playlist.widget(), &sender);
        // Videos dropped on the panel join the list; dropped on the video
        // they replace it (the window's own drop target, below).
        {
            let sender = sender.clone();
            big_app_kit::widgets::add_file_drop_target(&playlist_panel.revealer, move |paths| {
                let paths: Vec<String> = paths
                    .iter()
                    .filter_map(|path| path.to_str().map(str::to_string))
                    .collect();
                if paths.is_empty() {
                    return false;
                }
                sender.input(VideoShellInput::EnqueueFiles(paths));
                true
            });
        }
        let notice = MediaNotice::new();

        // Keep-Above toggle. No portable Wayland keep-above exists, so toggle it
        // via the compositor (KWin global shortcut, falling back to wmctrl). Both
        // run as argv arrays with no shell and no untrusted input.
        let keep_above_toggle_button = gtk::ToggleButton::builder()
            .icon_name("view-pin-symbolic")
            .tooltip_text(i18n("Keep Above"))
            .css_classes(["osd-btn"])
            .build();
        keep_above_toggle_button
            .update_property(&[gtk::accessible::Property::Label(&i18n("Keep Above"))]);
        keep_above_toggle_button.connect_toggled(|_button| toggle_keep_above());

        // Open popover button: a real GtkButton exposes AT-SPI DoAction while
        // the popover rows keep named, activatable file-opening actions.
        let open_label = i18n("Open");
        let open_popover_button = build_flat_action_popover_button(
            &BigHamburgerMenuSpec::new(open_label)
                .icon_name("document-open-symbolic")
                .css_classes(["osd-btn"])
                .items(open_menu_items()),
        );
        let open_menu_root = open_popover_button.root().clone();
        let open_menu_popover = open_popover_button.popover().clone();

        // Two separate sidebar entry points: Files (folder browser) and Playlist
        // (the play queue). Each reveals the sidebar in its mode; clicking the
        // active one hides it. Not a radio pair: both off hides the sidebar.
        let files_sidebar_toggle_button = gtk::ToggleButton::builder()
            .icon_name("folder-symbolic")
            .tooltip_text(i18n("Files"))
            .css_classes(["osd-btn"])
            .build();
        files_sidebar_toggle_button
            .update_property(&[gtk::accessible::Property::Label(&i18n("Files"))]);
        let playlist_sidebar_toggle_button = gtk::ToggleButton::builder()
            .icon_name("view-list-symbolic")
            .tooltip_text(i18n("Playlist"))
            .css_classes(["osd-btn"])
            .build();
        playlist_sidebar_toggle_button
            .update_property(&[gtk::accessible::Property::Label(&i18n("Playlist"))]);

        // Guard so programmatic `set_active` doesn't re-enter the handlers.
        let toggles_updating = Rc::new(Cell::new(false));
        let active_sidebar_mode = Rc::new(Cell::new(SidebarMode::Files));
        // The shell decides what the buttons show: a click only asks.
        for (button, mode) in [
            (&files_sidebar_toggle_button, SidebarMode::Files),
            (&playlist_sidebar_toggle_button, SidebarMode::Playlist),
        ] {
            let sender = sender.clone();
            let updating = toggles_updating.clone();
            button.connect_toggled(move |_| {
                if !updating.get() {
                    sender.input(match mode {
                        SidebarMode::Files => VideoShellInput::ToggleFilesSidebar,
                        SidebarMode::Playlist => VideoShellInput::TogglePlaylistSidebar,
                    });
                }
            });
        }
        // Put away by Escape or a click on the video: release both buttons.
        playlist_panel.revealer.connect_reveal_child_notify({
            let files_sidebar_toggle_button = files_sidebar_toggle_button.downgrade();
            let playlist_sidebar_toggle_button = playlist_sidebar_toggle_button.downgrade();
            let updating = toggles_updating.clone();
            move |revealer| {
                if revealer.reveals_child() {
                    return;
                }
                updating.set(true);
                if let Some(button) = files_sidebar_toggle_button.upgrade() {
                    button.set_active(false);
                }
                if let Some(button) = playlist_sidebar_toggle_button.upgrade() {
                    button.set_active(false);
                }
                updating.set(false);
            }
        });

        // Shared media-viewer chrome (header + accessible primary menu + the
        // OSD reveal); see big_media_components::surface. The video
        // player keeps mpv/offload/welcome/buffering state app-local; shared
        // chrome owns the repeatable header, bottom revealer, and sidebar split.
        chrome.add_header_start(&keep_above_toggle_button);
        chrome.add_header_start(&open_menu_root);
        chrome.add_header_start(&files_sidebar_toggle_button);
        chrome.add_header_start(&playlist_sidebar_toggle_button);

        let win_actions = install_window_actions(&root, &sender);

        // Header is an OSD overlay over edge-to-edge video, not an
        // AdwToolbarView top bar: wrapping the video surface in a
        // ToolbarView breaks GraphicsOffload subsurface presentation (the video
        // rendered but presented at ~1 fps). With the video filling the split's
        // content directly, the offload subsurface presents at full rate.
        //
        // Wrap the headerbar in a box carrying the custom `.osd-header` class
        // (not the libadwaita built-in `.osd` class). `.osd` forces light-on-dark
        // regardless of theme, which in the light theme leaves the header icons
        // (incl. the hamburger menu button) light-on-light and unreadable. The
        // app-owned `apply_appearance` CSS styles `.osd-header headerbar` with
        // theme-aware colors (the `.osd-btn` header buttons get
        // `@player_icon_color`), so both light and dark themes stay readable.
        media_stack.install_chrome(&chrome, BigMediaChromePlacement::Header);

        // OSD auto-hide: reveal on pointer motion over the video, hide after
        // `OSD_HIDE_DELAY_MS` of inactivity or on pointer-leave. The welcome screen is up at init, so the OSD
        // never auto-hides until the first file loads.
        let trim_active = Rc::new(Cell::new(false));
        let osd = OsdState::new(
            &main_overlay,
            chrome.header_revealer(),
            chrome.bottom_revealer(),
            // The TV menu too: its popover taking the focus is not the window
            // losing it, and the header must not fold away from under it.
            vec![open_menu_popover]
                .into_iter()
                .chain(tv_pill.popover())
                .collect(),
            controls.model().popovers(),
            !opening_files,
            trim_active.clone(),
        );
        wire_osd_autohide(&main_overlay, &osd);
        // The settings panel sits over the video, clear of header and controls,
        // which stay shown while it is open.
        // Attached, the TV already plays what the player holds: no swap.
        let tv_card = crate::shared::tv_cast::TvCard::new("settings-panel", false, false, true, {
            let sender = sender.clone();
            move |action| sender.input(VideoShellInput::Tv(action))
        });
        main_overlay.add_overlay(&tv_card.root);
        let panel = super::panel::SettingsPanel::new(&sender);
        main_overlay.add_overlay(&panel.revealer);
        main_overlay.add_overlay(&playlist_panel.revealer);
        for revealer in [&panel.revealer, &playlist_panel.revealer] {
            let revealer = revealer.downgrade();
            osd.keep_shown_while(move || revealer.upgrade().is_some_and(|r| r.reveals_child()));
        }
        // Collapse the OSD when the window loses focus (alt-tab to another
        // app), like other video players, as well as on the idle timer.
        wire_osd_focus_out(&main_overlay, &osd);
        // Pointer gestures over the video:
        // double-click → fullscreen, right-click → context menu, drag → move
        // the window (when not fullscreen).
        wire_video_input(&root, &main_overlay, &sender);
        // Window-level keyboard shortcuts (space/seek/volume/fullscreen/…).
        super::keyboard::wire_keyboard_shortcuts(&root, &mpv, &tv, &settings, &sender);
        // Trim-mode keys (I / O / M / Esc): a capture-phase controller that
        // only acts while trim mode is active, so it never shadows the normal
        // bindings on those keys outside trim mode.
        super::keyboard::wire_trim_keys(&root, trim_active.clone(), &sender);

        // Static idle background behind the welcome screen (gl surface hidden so
        // it shows through). Shown below once the display is known; restored on
        // the first `FileLoaded`. The blurred desktop wallpaper is fetched off
        // the main loop (idle_wallpaper::spawn_apply) and swapped in when ready.
        let idle_bg = Rc::new(IdleBackground {
            video_bg: video_bg.clone(),
            gl_area: gl_area.clone(),
            provider: RefCell::new(None),
            blurred_path: RefCell::new(None),
            is_shown: Cell::new(false),
            tv_source: RefCell::new(None),
            tv_picture: RefCell::new(None),
        });

        // Messages take a row above the content instead of covering the video.
        root.append(notice.widget());
        root.append(&main_overlay);

        // Show the idle background now (welcome is up). Hidden on first
        // `FileLoaded` so the video surface becomes visible again. Kick off the
        // wallpaper detection + blur so the static gradient is replaced by the
        // blurred desktop wallpaper once it is cached.
        if !opening_files && let Some(display) = gtk::gdk::Display::default() {
            idle_bg.show(&display);
        }
        super::idle_wallpaper::spawn_apply(idle_bg.clone());

        // mpv option strings, read once.
        let mut custom_mpv_options = settings.get_string(keys::CUSTOM_MPV_OPTIONS).clone();
        if settings.get_bool(keys::GPU_DUMB_MODE) {
            if !custom_mpv_options.is_empty() {
                custom_mpv_options.push(',');
            }
            custom_mpv_options.push_str("gpu-dumb-mode=yes");
        }
        let audio_output = settings.get_string(keys::AUDIO_OUTPUT).clone();
        let initial_volume = settings.get_double(keys::VOLUME) * 100.0;

        let fast_render_context_slot: FastRenderContextSlot = Rc::new(RefCell::new(None));
        let video_backend: Rc<RefCell<Option<VideoBackend>>> = Rc::new(RefCell::new(None));
        let video_start = VideoStart::default();

        // mpv/GL comes up when the CONTENT realizes (it needs only the display +
        // overlay/gl_area, not a window), so it works in any mount point.
        wire_realize(
            &root,
            &main_overlay,
            &gl_area,
            &sw_picture,
            &mpv,
            &fast_render_context_slot,
            &video_backend,
            &video_start,
            &sender,
            VideoRealizeOptions {
                custom_mpv_options,
                audio_output,
                initial_volume,
                volume_max: i64::from(settings.get_int_or(keys::VOLUME_MAX, 150)),
                render_mode: settings.get_string(keys::RENDER_MODE).clone(),
                settings: settings.clone(),
                notice: notice.clone(),
            },
        );

        // Publish the shell input so the application `open` handler can deliver
        // files opened from the desktop / CLI.
        let shell_input_token = set_shell_input(sender.input_sender().clone());
        root.connect_unrealize(move |_| clear_shell_input(shell_input_token));

        // Apply the saved OSD palette + layout CSS now into the shared
        // display-wide provider slot; the Appearance
        // dialog re-applies live through the same slot.
        reapply_theme(&settings);

        // Window aspect-ratio snap: resize the window to the video's aspect on
        // load and on manual resize (debounced), so no letterbox bars remain.
        // Resolves the window lazily from the content.
        let aspect_snap = Rc::new(AspectSnap {
            content: root.downgrade(),
            settings: settings.clone(),
            mpv: mpv.clone(),
            gl_area: gl_area.clone(),
            playlist_panel: playlist_panel.revealer.downgrade(),
            aspect: Cell::new(0.0),
            resizing: Cell::new(false),
            resize_gen: Cell::new(0),
        });

        // Window-needing wiring (close → mpv teardown, the default-size notify
        // that drives the aspect snap) is deferred to first realize, when the
        // content is actually in a window (it has none at init; the mount point
        // supplies it). Runs once.
        {
            let mpv = mpv.clone();
            let settings = settings.clone();
            let snap = aspect_snap.clone();
            let done = Cell::new(false);
            root.connect_realize(move |root| {
                if done.replace(true) {
                    return;
                }
                let Some(window) = root.root().and_downcast::<adw::ApplicationWindow>() else {
                    return;
                };
                wire_close(&window, &mpv, &settings);
                {
                    let snap = snap.clone();
                    window.connect_default_width_notify(move |_| snap.schedule());
                }
                {
                    let snap = snap.clone();
                    window.connect_default_height_notify(move |_| snap.schedule());
                }
            });
        }

        let mpris_state = Arc::new(std::sync::Mutex::new(
            big_app_kit::mpris::PlayerState::default(),
        ));
        let mpris = super::mpris::build_mpris_service(&sender, &mpris_state, &settings);

        ComponentParts {
            model: Self {
                controls,
                playlist,
                playlist_panel,
                sidebar_mode: active_sidebar_mode,
                files_sidebar_button: files_sidebar_toggle_button,
                playlist_sidebar_button: playlist_sidebar_toggle_button,
                sidebar_toggle_updating: toggles_updating,
                iptv,
                main_overlay,
                welcome_overlay: RefCell::new(Some(welcome)),
                playlist_paths: Vec::new(),
                shuffle_on_load: None,
                adjustments: Vec::new(),
                adjustments_pill,
                hdr_hint,
                last_reset: None,
                panel,
                tv,
                tv_card,
                tv_pill,
                tv_menu,
                tv_catch_up: false,
                tv_last: None,
                tv_was_released: false,
                tv_follow: None,
                tv_here: None,
                playlist_second: -1,
                mpv,
                video_start,
                duration_secs: 0.0,
                settings,
                video_transform,
                osd,
                idle_bg,
                buffering_overlay,
                notice,
                screenshot_owner: Arc::new(std::sync::atomic::AtomicBool::new(true)),
                win_actions,
                sleep_timer_source: None,
                sleep_timer_remaining: 0,
                sleep_timer_action: String::new(),
                sleep_timer_volume: 100.0,
                aspect_snap,
                shell_input_token,
                trim,
                trim_play_selection: Rc::new(Cell::new(false)),
                trim_ranges: Rc::new(RefCell::new(Vec::new())),
                trim_active,
                mpris,
                mpris_state,
                mpris_track_seq: 0,
                playing: false,
                screen_inhibit: None,
            },
            widgets: (),
        }
    }

    fn update(
        &mut self,
        shell_input: Self::Input,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        // Panic boundary (module contract): a panic in message handling reaps
        // this module's window cleanly (host closes it) instead of orphaning a
        // dead window; the host process and sibling modules survive.
        big_app_kit::containment::contain_embedded_update(root, "video-player", || {
            let adw_window = root.root().and_downcast::<adw::ApplicationWindow>();
            let window_ref = adw_window.as_ref();
            let gtk_window = root.root().and_downcast::<gtk::Window>();
            let gtk_window_ref = gtk_window.as_ref();
            match shell_input {
                VideoShellInput::Player(event) => {
                    // After the shell reacts (a load-time shuffle reorders the
                    // playlist), so MPRIS sees the final position.
                    self.on_player_event(event.clone(), window_ref);
                    self.mirror_mpris(&event);
                }
                VideoShellInput::Mpris(command) => {
                    self.handle_mpris_command(command, &sender, gtk_window_ref);
                }
                VideoShellInput::Controls(out) => self.on_controls(out, window_ref),
                VideoShellInput::Playlist(out) => self.on_playlist(out, &sender),
                VideoShellInput::EnterTrimMode => self.enter_trim_mode(),
                VideoShellInput::ExitTrimMode => self.exit_trim_mode(),
                VideoShellInput::TrimBar(out) => self.on_trim_bar(out, window_ref),
                VideoShellInput::TrimMark => self.trim_mark(),
                VideoShellInput::ExtractAudio => {
                    self.open_in_converter(converter_handoff::AUDIO_CONVERTER, &[]);
                }
                VideoShellInput::Convert => {
                    self.open_in_converter(converter_handoff::VIDEO_CONVERTER, &[]);
                }
                VideoShellInput::Iptv(out) => VideoShell::on_iptv(out, &sender),
                VideoShellInput::ShowIptvBrowser => {
                    if let Some(root) = window_ref {
                        self.show_iptv_browser(root);
                    }
                }
                VideoShellInput::TogglePanel => self.toggle_panel(&sender),
                VideoShellInput::ShowPanel(page) => self.show_panel(page, true, &sender),
                VideoShellInput::PanelNavigate(page) => self.show_panel(page, false, &sender),
                VideoShellInput::PanelBack => self.panel_back(false, &sender),
                VideoShellInput::PanelEscape => self.panel_back(true, &sender),
                VideoShellInput::ClosePanel => self.close_panel(&sender),
                VideoShellInput::HidePlaylistPanel => self.playlist_panel.hide(),
                VideoShellInput::EnqueueFiles(paths) => self.enqueue_files(&paths),
                VideoShellInput::ShowYtdlQuality => {
                    if let Some(root) = window_ref {
                        self.show_ytdl_quality_dialog(root);
                    }
                }
                VideoShellInput::ShowMediaInfo => {
                    if let Some(root) = window_ref {
                        self.show_media_details(root);
                    }
                }
                VideoShellInput::ShowKeyboardShortcuts => {
                    if let Some(root) = window_ref {
                        show_video_keyboard_shortcuts_dialog(root, &self.settings);
                    }
                }
                VideoShellInput::ShowOpenFile => {
                    if let Some(root) = gtk_window_ref {
                        Self::show_open_file_dialog(root);
                    }
                }
                VideoShellInput::ShowOpenUrl => {
                    if let Some(root) = window_ref {
                        Self::show_open_url_dialog(root);
                    }
                }
                VideoShellInput::ShowRecent => {
                    self.show_panel(super::panel::PanelPage::Recent, true, &sender);
                }
                VideoShellInput::ShowOpenDisc => {
                    if let Some(root) = window_ref {
                        Self::show_open_disc_dialog(root);
                    }
                }
                VideoShellInput::ToggleFilesSidebar => {
                    self.toggle_sidebar_mode(SidebarMode::Files);
                }
                VideoShellInput::TogglePlaylistSidebar => {
                    self.toggle_sidebar_mode(SidebarMode::Playlist);
                }
                VideoShellInput::ShowPreferences => {
                    if let Some(root) = window_ref {
                        self.show_preferences(root);
                    }
                }
                VideoShellInput::PreferencesClosed => self.sync_screen_inhibit(window_ref),
                VideoShellInput::ShowAppearance => {
                    if let Some(root) = window_ref {
                        self.show_appearance(root);
                    }
                }
                VideoShellInput::ShowRestoreDefaults => {
                    if let Some(root) = window_ref {
                        Self::show_restore_defaults_dialog(root, || {});
                    }
                }
                // A restart picks up the new values: mpv is configured at startup.
                VideoShellInput::RestoreDefaultsConfirmed => self.settings.reset_to_defaults(),
                VideoShellInput::OpenFiles(paths) => VideoShell::open_files(&paths, &sender),
                VideoShellInput::PlaylistResolved {
                    entries,
                    start,
                    folder,
                } => self.apply_open(entries, start, folder),
                VideoShellInput::ShowGotoTime => {
                    if let Some(root) = window_ref {
                        self.show_goto_time_dialog(root);
                    }
                }
                VideoShellInput::Screenshot => self.take_screenshot(&sender),
                VideoShellInput::ScreenshotQueueFailed => self.notify(&i18n("Screenshot failed")),
                VideoShellInput::SetChapter(idx) => self.set_chapter(idx),
                VideoShellInput::SleepTimerStart { minutes, action } => {
                    self.start_sleep_timer(sleep_minutes_to_secs(minutes), action, &sender);
                    self.refresh_adjustments(&sender);
                }
                VideoShellInput::SleepTimerCancel => {
                    self.cancel_sleep_timer();
                    self.refresh_adjustments(&sender);
                    self.refresh_panel_home(&sender);
                }
                VideoShellInput::RefreshAdjustments => self.refresh_adjustments(&sender),
                VideoShellInput::ResetAdjustments(adjustments) => {
                    self.reset_adjustments(adjustments, &sender);
                }
                VideoShellInput::ResetAllAdjustments => {
                    let all = self
                        .adjustments
                        .iter()
                        .filter(|a| !matches!(a, adjustments::Adjustment::SleepTimer(_)))
                        .cloned()
                        .collect();
                    self.reset_adjustments(all, &sender);
                }
                VideoShellInput::SuggestHdr(path) => self.suggest_hdr(&path),
                VideoShellInput::RestoreAdjustments(adjustments) => {
                    self.restore_adjustments(&adjustments, &sender);
                }
                VideoShellInput::SleepTick => self.sleep_tick(),
                VideoShellInput::TogglePlayback => {
                    if let Some(state) = self.tv.attached() {
                        self.tv
                            .control(if state.paused { "play" } else { "pause" }, 0.0);
                    } else if let Some(mpv) = self.mpv.borrow().as_ref() {
                        mpv.toggle_pause();
                    }
                }
                VideoShellInput::TvChanged => self.on_tv_changed(),
                VideoShellInput::Tv(action) => self.on_tv_card(action),
                VideoShellInput::ToggleFullscreen => {
                    if let Some(root) = window_ref {
                        let _ = toggle_surface_fullscreened(root);
                    }
                }
                VideoShellInput::Escape => {
                    if self.playlist_panel.shows() {
                        self.playlist_panel.hide();
                    } else if self.panel.is_open() {
                        self.panel_back(true, &sender);
                    } else if let Some(root) = window_ref
                        && root.is_fullscreen()
                    {
                        root.unfullscreen();
                    }
                }
            }
            if let Some(mpris) = &self.mpris {
                mpris.sync();
            }
        });
    }
}

impl Drop for VideoShell {
    /// Remove the armed sleep-timer countdown source so it does not outlive the
    /// shell (no leaked `glib::timeout`) and clear the global input broker.
    ///
    /// Do not force-drop the live mpv backend here: GTK widgets still own
    /// render-context callbacks during unrealize/finalize, and those callbacks
    /// must release their mpv render context before the shared mpv handle is
    /// destroyed.
    fn drop(&mut self) {
        clear_shell_input(self.shell_input_token);
        self.screenshot_owner
            .store(false, std::sync::atomic::Ordering::Release);
        if let Some(source) = self.sleep_timer_source.take() {
            big_gtk_kit::source::remove(source);
        }
        if let Some(cookie) = self.screen_inhibit.take()
            && let Some(app) = gtk::gio::Application::default().and_downcast::<gtk::Application>()
        {
            app.uninhibit(cookie);
        }
    }
}

#[cfg(all(test, not(miri)))]
fn build_open_menu_model() -> gio::Menu {
    build_flat_menu_model(&open_menu_items())
}

fn open_menu_items() -> Vec<BigMenuActionItem> {
    vec![
        BigMenuActionItem::new(i18n("Open File…"), "win.open-file"),
        BigMenuActionItem::new(i18n("Open Recent"), "win.open-recent"),
        BigMenuActionItem::new(i18n("Open URL…"), "win.open-url"),
        BigMenuActionItem::new(i18n("Open Disc…"), "win.open-disc"),
        BigMenuActionItem::new(i18n("TV Channels (IPTV)…"), "win.iptv"),
    ]
}

#[cfg(test)]
fn open_menu_action_names() -> Vec<String> {
    open_menu_items()
        .into_iter()
        .map(|menu_item| menu_item.action)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_menu_uses_flat_action_items() {
        assert_menu_actions_include(
            &open_menu_action_names(),
            &[
                "win.open-file",
                "win.open-recent",
                "win.open-url",
                "win.open-disc",
                "win.iptv",
            ],
        );
    }

    #[cfg(not(miri))]
    #[test]
    fn open_menu_is_flat() {
        let menu = build_open_menu_model();
        for index in 0..menu.n_items() {
            assert!(menu.item_link(index, "section").is_none());
            assert!(menu.item_link(index, "submenu").is_none());
        }
    }

    fn assert_menu_actions_include(actions: &[String], expected: &[&str]) {
        for action in expected {
            assert!(
                actions.iter().any(|actual| actual == action),
                "missing menu action {action}; actions={actions:?}"
            );
        }
    }
}
