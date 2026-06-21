use super::actions::{
    build_welcome_screen, install_window_actions, toggle_keep_above, wire_sidebar_toggle,
};
#[cfg(feature = "chromecast")]
use super::cast_snapshot::build_cast_filter_snapshot;
use super::dialogs::reapply_theme;
use super::input::{clear_shell_input, set_shell_input};
use super::input_gestures::{wire_background_video_input, wire_osd_autohide, wire_video_input};
use super::render::{wire_close, wire_realize, RealizeOpts, RenderCtx};
use super::*;

impl Component for VideoShell {
    type Init = VideoShellMode;
    type Input = VideoShellInput;
    type Output = ();
    type CommandOutput = ();
    // Root is the mountable CONTENT (the toast overlay), not a window: the window
    // chrome + window-scoped setup are supplied / resolved by the mount point
    // (standalone boot, host module, or pane) — RESTRUCTURE ADR D14.
    type Root = adw::ToastOverlay;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::ToastOverlay::new()
    }

    fn init(
        mode: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let settings = Settings::for_video();
        let is_background = mode.is_background();

        // App stylesheet + icon path (the god-object loaded these; the shell
        // owns them now). Scoped CSS class for black bg + rounded corners.
        if let Some(display) = gtk::gdk::Display::default() {
            let provider = gtk::CssProvider::new();
            provider.load_from_resource(&format!(
                "{}/style.css",
                crate::config::video::RESOURCE_PREFIX
            ));
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            gtk::IconTheme::for_display(&display)
                .add_resource_path(&format!("{}/icons", crate::config::video::RESOURCE_PREFIX));
        }
        root.add_css_class("big-video-player");
        if is_background {
            root.add_css_class("big-video-background");
        }

        // Shared mpv handle: created here so the controls-bar popovers can read
        // it at call time, and so the realize handler can fill it.
        let mpv: Rc<RefCell<Option<Arc<MpvBackend>>>> = Rc::new(RefCell::new(None));
        let video_transform = media_adjust::VideoTransformState::default();

        // Interactive trim bar — built first so its widget can be mounted INSIDE
        // the controls bar (shown in place of the transport row in trim mode, not
        // a separate bar). Its typed outputs route back into the shell.
        let trim = trim::VideoTrimBar::builder()
            .launch(())
            .forward(sender.input_sender(), VideoShellInput::TrimBar);

        // OSD controls bar (app-local; recovers the old `controls_box`). The trim
        // bar rides inside it (transport ↔ trim stack).
        let controls = VideoControlsBar::builder()
            .launch(VideoControlsBarInit {
                settings: settings.clone(),
                mpv: mpv.clone(),
                trim_content: trim.widget().clone().upcast::<gtk::Widget>(),
            })
            .forward(sender.input_sender(), VideoShellInput::Controls);
        controls.emit(VideoControlsBarInput::SetControlsSensitive(false));

        // Folder-browser playlist sidebar (folder scan = the playlist).
        let playlist = VideoBrowserSidebar::builder()
            .launch(settings.clone())
            .forward(sender.input_sender(), VideoShellInput::Playlist);

        // IPTV browser (video-local). Held detached as a dialog child; play
        // intents route back to the shell → mpv.
        let iptv = VideoIptvBrowser::builder()
            .launch(())
            .forward(sender.input_sender(), VideoShellInput::Iptv);

        // Cast device picker (video-local UI over the shared CastManager
        // engine). Held detached as a dialog child; start/stop reported back.
        #[cfg(feature = "chromecast")]
        let cast = VideoCastDialog::builder()
            .launch(VideoCastDialogInit {
                manager: Arc::new(CastManager::new()),
                mpv: mpv.clone(),
                filter_snapshot: {
                    let mpv = mpv.clone();
                    let settings = settings.clone();
                    let video_transform = video_transform.clone();
                    Rc::new(move || build_cast_filter_snapshot(&mpv, &settings, &video_transform))
                },
            })
            .forward(sender.input_sender(), VideoShellInput::Cast);

        // Shared re-encode options (container/codec/quality) — same vocabulary as
        // the full converter, shown in the gear settings dialog (built lazily).
        let trim_encode = big_converter_ui::encode_options::EncodeOptions::new();

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

        let main_overlay = gtk::Overlay::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&video_bg)
            .build();
        main_overlay.add_overlay(&gl_area);
        main_overlay.set_clip_overlay(&gl_area, true);
        main_overlay.set_measure_overlay(&gl_area, false);

        // Software (no-GL) video surface: hidden until the GL render context
        // fails to come up (headless software compositor, GL-less VM). The SW
        // render path paints mpv frames here as memory textures (cairo path).
        let sw_picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .hexpand(true)
            .vexpand(true)
            .visible(false)
            .build();
        main_overlay.add_overlay(&sw_picture);
        main_overlay.set_clip_overlay(&sw_picture, true);
        main_overlay.set_measure_overlay(&sw_picture, false);

        // The trim controls live INSIDE the controls bar (transport ↔ trim stack);
        // the bottom OSD is just the controls bar, as before.
        let controls_revealer = gtk::Revealer::builder()
            .valign(gtk::Align::End)
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .reveal_child(!is_background)
            .child(controls.widget())
            .build();
        // Slim, non-blocking export-progress strip: a shared `BigProgressRow`
        // (title · bar · percent/speed/ETA · Cancel) on a flat status bar docked
        // DIRECTLY ABOVE the transport — so the user keeps watching the video and
        // follows the cut progress in place (NOT the converter's full-cover page).
        let trim_progress_row = BigProgressRow::new(
            &big_relm4_components::list::progress_row::BigProgressRowSpec::indeterminate(
                i18n("Generating cut…"),
                String::new(),
            )
            .with_action(
                big_relm4_components::list::progress_row::BigProgressRowActionSpec::new(
                    "process-stop-symbolic",
                    i18n("Cancel"),
                ),
            ),
        );
        if let Some(cancel) = trim_progress_row.action_button() {
            cancel.connect_clicked(glib::clone!(
                #[strong]
                sender,
                move |_| sender.input(VideoShellInput::TrimCancelExport)
            ));
        }
        let trim_progress_strip = gtk::Box::new(gtk::Orientation::Vertical, 0);
        trim_progress_strip.add_css_class("trim-progress");
        trim_progress_strip.append(trim_progress_row.root());
        let trim_progress_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .reveal_child(false)
            .child(&trim_progress_strip)
            .build();
        // One bottom stack: the progress strip sits above the transport so it
        // reads as an extension of the bottom controls, never covering the video.
        let bottom_stack = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bottom_stack.set_valign(gtk::Align::End);
        bottom_stack.append(&trim_progress_revealer);
        bottom_stack.append(&controls_revealer);
        // Re-encode options (container/codec/quality) live in the gear dialog,
        // not an over-screen panel; `trim_encode.group()` is parented there on
        // first open.
        if !is_background {
            main_overlay.add_overlay(&bottom_stack);
            main_overlay.set_measure_overlay(&bottom_stack, false);
        }

        // Welcome screen: shown until the first file loads (hidden on
        // `FileLoaded`). Mirrors the old `setup_welcome_screen` overlay.
        let welcome = if is_background {
            None
        } else {
            let welcome = build_welcome_screen(&sender);
            main_overlay.add_overlay(&welcome);
            main_overlay.set_measure_overlay(&welcome, false);
            Some(welcome)
        };

        // Buffering spinner: centered overlay, hidden until `Buffering(true)`.
        // Mirrors the old `status_overlay`/`buffering_spinner`.
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
        main_overlay.add_overlay(&buffering_overlay);
        main_overlay.set_measure_overlay(&buffering_overlay, false);

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

        // Sidebar reveal: collapsed overlay (playlist) over the video content.
        // Hidden by default; a header toggle/`OpenFiles` reveals it. The sidebar
        // widget is wrapped with a draggable handle on its inner (content) edge
        // so the user can widen it (bigger grid previews); the handle adjusts
        // `max_sidebar_width` live (matches the old player).
        let drag_handle = gtk::Separator::new(gtk::Orientation::Vertical);
        drag_handle.set_width_request(6);
        drag_handle.add_css_class("sidebar-drag-handle");
        drag_handle.set_cursor_from_name(Some("col-resize"));
        let sidebar_outer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let pl_widget = playlist.widget();
        pl_widget.set_hexpand(true);
        sidebar_outer.append(pl_widget);
        sidebar_outer.append(&drag_handle);

        let split_view = adw::OverlaySplitView::builder()
            .sidebar(&sidebar_outer)
            .content(&main_overlay)
            .show_sidebar(false)
            .min_sidebar_width(260.0)
            .max_sidebar_width(480.0)
            // fraction 1.0 → actual width == max_sidebar_width (clamped), so the
            // drag handle resizing max actually resizes the sidebar.
            .sidebar_width_fraction(1.0)
            .build();

        // Drag the handle to resize: a left (start) sidebar widens as the inner
        // edge moves right (+x). Clamped 260..900; not persisted (per old).
        let resize = gtk::GestureDrag::new();
        let resize_start_width = Rc::new(Cell::new(0.0_f64));
        // Weak split_view: the gesture lives on a handle INSIDE the split
        // view — strong captures here pinned the whole content tree after
        // close (window finalized, split subtree leaked).
        resize.connect_drag_begin({
            let split_view = split_view.downgrade();
            let resize_start_width = resize_start_width.clone();
            move |_, _, _| {
                if let Some(split_view) = split_view.upgrade() {
                    resize_start_width.set(split_view.max_sidebar_width());
                }
            }
        });
        resize.connect_drag_update({
            let split_view = split_view.downgrade();
            move |_, offset_x, _| {
                let Some(split_view) = split_view.upgrade() else {
                    return;
                };
                let new_max = (resize_start_width.get() + offset_x).clamp(260.0, 900.0);
                split_view.set_max_sidebar_width(new_max);
            }
        });
        drag_handle.add_controller(resize);

        let header = adw::HeaderBar::new();

        // Keep-Above pin toggle (old `pin_btn`). No portable Wayland keep-above
        // exists; mirror the god-object by toggling via the compositor (KWin
        // global shortcut, falling back to wmctrl). Both via argv arrays — no
        // shell, no untrusted input.
        let pin_btn = gtk::ToggleButton::builder()
            .icon_name("view-pin-symbolic")
            .css_classes(["osd-btn"])
            .build();
        pin_btn.update_property(&[gtk::accessible::Property::Label(&i18n("Keep Above"))]);
        pin_btn.connect_toggled(|_btn| toggle_keep_above());
        header.pack_start(&pin_btn);

        // Open menu-button (old `open_btn`): Open File… / Open Recent / Open URL…
        let open_menu = gio::Menu::new();
        open_menu.append(Some(&i18n("Open File…")), Some("win.open-file"));
        open_menu.append(Some(&i18n("Open Recent")), Some("win.open-recent"));
        open_menu.append(Some(&i18n("Open URL…")), Some("win.open-url"));
        open_menu.append(Some(&i18n("Open Disc…")), Some("win.open-disc"));
        let open_btn = gtk::MenuButton::builder()
            .icon_name("document-open-symbolic")
            .menu_model(&open_menu)
            .css_classes(["osd-btn"])
            .build();
        open_btn.update_property(&[gtk::accessible::Property::Label(&i18n("Open"))]);
        header.pack_start(&open_btn);

        // Two separate sidebar entry points: Files (folder browser) and Playlist
        // (the play queue). Each reveals the sidebar in its mode; clicking the
        // active one hides it. Not a radio pair — both off = sidebar hidden.
        let files_btn = gtk::ToggleButton::builder()
            .icon_name("folder-symbolic")
            .css_classes(["osd-btn"])
            .build();
        files_btn.update_property(&[gtk::accessible::Property::Label(&i18n("Files"))]);
        let list_btn = gtk::ToggleButton::builder()
            .icon_name("view-list-symbolic")
            .css_classes(["osd-btn"])
            .build();
        list_btn.update_property(&[gtk::accessible::Property::Label(&i18n("Playlist"))]);

        // Guard so programmatic `set_active` (deactivating the sibling) doesn't
        // re-enter the toggled handlers.
        let toggles_updating = Rc::new(Cell::new(false));
        let pl_sender = playlist.sender().clone();
        wire_sidebar_toggle(
            &files_btn,
            &list_btn,
            SidebarMode::Files,
            &split_view,
            &toggles_updating,
            &pl_sender,
        );
        wire_sidebar_toggle(
            &list_btn,
            &files_btn,
            SidebarMode::Playlist,
            &split_view,
            &toggles_updating,
            &pl_sender,
        );
        // Keep the toggles in sync when the overlay is dismissed externally
        // (click-outside / Esc collapses an AdwOverlaySplitView) → release both.
        split_view.connect_show_sidebar_notify({
            let files_btn = files_btn.downgrade();
            let list_btn = list_btn.downgrade();
            let updating = toggles_updating.clone();
            move |sv| {
                if updating.get() || sv.shows_sidebar() {
                    return;
                }
                updating.set(true);
                if let Some(b) = files_btn.upgrade() {
                    b.set_active(false);
                }
                if let Some(b) = list_btn.upgrade() {
                    b.set_active(false);
                }
                updating.set(false);
            }
        });
        header.pack_start(&files_btn);
        header.pack_start(&list_btn);

        // Hamburger menu (old `menu_btn`): only the items whose dialogs are
        // actually ported. Not-yet-ported entries are deliberately omitted (a
        // dead menu item is worse than an absent one).
        // Labelled sections (gio renders the label as a small-caps header in the
        // popover) so the user scans by category instead of reading 20 items one
        // by one. The old single 8-item "Media" grab-bag is split into Audio /
        // Video / Sources by what the entry actually controls.
        let menu = gio::Menu::new();
        let playback = gio::Menu::new();
        playback.append(Some(&i18n("Go to Time…")), Some("win.goto-time"));
        playback.append(Some(&i18n("Chapters")), Some("win.chapters"));
        playback.append(Some(&i18n("Screenshot")), Some("win.screenshot"));
        playback.append(Some(&i18n("Sleep Timer…")), Some("win.sleep-timer"));
        menu.append_section(Some(&i18n("Playback")), &playback);

        let audio = gio::Menu::new();
        audio.append(Some(&i18n("Equalizer")), Some("win.show-equalizer"));
        audio.append(
            Some(&i18n("Audio & Subtitle Delay")),
            Some("win.show-delay"),
        );
        menu.append_section(Some(&i18n("Audio")), &audio);

        let video = gio::Menu::new();
        // Renamed for clarity: "Image & Video" was really the colour/picture
        // adjustments; "Online Video Quality" is the stream-quality picker.
        video.append(
            Some(&i18n("Picture Adjustments")),
            Some("win.show-image-adjust"),
        );
        video.append(Some(&i18n("Video Filters")), Some("win.filters"));
        video.append(Some(&i18n("Stream Quality")), Some("win.ytdl-quality"));
        menu.append_section(Some(&i18n("Video")), &video);

        let sources = gio::Menu::new();
        sources.append(Some(&i18n("IPTV")), Some("win.iptv"));
        #[cfg(feature = "chromecast")]
        sources.append(Some(&i18n("Cast to Device")), Some("win.cast"));
        sources.append(Some(&i18n("Media Info")), Some("win.media-info"));
        menu.append_section(Some(&i18n("Sources")), &sources);

        // Edit & export: reuse the converter engine in-process — cut, extract
        // audio, or re-encode the playing file without leaving the player.
        let export = gio::Menu::new();
        export.append(Some(&i18n("Cut Video…")), Some("win.trim"));
        let extract = gio::Menu::new();
        extract.append(Some(&i18n("MP3")), Some("win.extract-audio-mp3"));
        extract.append(Some(&i18n("M4A")), Some("win.extract-audio-m4a"));
        extract.append(Some(&i18n("FLAC")), Some("win.extract-audio-flac"));
        export.append_submenu(Some(&i18n("Extract Audio")), &extract);
        let convert = gio::Menu::new();
        convert.append(Some(&i18n("MP4")), Some("win.convert-mp4"));
        convert.append(Some(&i18n("MKV")), Some("win.convert-mkv"));
        convert.append(Some(&i18n("WebM")), Some("win.convert-webm"));
        export.append_submenu(Some(&i18n("Convert to")), &convert);
        menu.append_section(Some(&i18n("Edit & Export")), &export);

        let settings_section = gio::Menu::new();
        settings_section.append(Some(&i18n("Preferences")), Some("win.preferences"));
        settings_section.append(Some(&i18n("Appearance")), Some("win.appearance"));
        settings_section.append(
            Some(&i18n("Restore Defaults")),
            Some("win.restore-defaults"),
        );
        menu.append_section(Some(&i18n("Settings")), &settings_section);

        let help = gio::Menu::new();
        help.append(
            Some(&i18n("Keyboard Shortcuts")),
            Some("win.keyboard-shortcuts"),
        );
        help.append(Some(&i18n("About")), Some("app.about"));
        menu.append_section(Some(&i18n("Help")), &help);
        let menu_btn = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .css_classes(["osd-btn"])
            .build();
        menu_btn.update_property(&[gtk::accessible::Property::Label(&i18n("Menu"))]);
        header.pack_end(&menu_btn);

        install_window_actions(&root, &sender);

        // Header is an OSD overlay over edge-to-edge video (matches the old
        // design), NOT an AdwToolbarView top bar: wrapping the video surface in a
        // ToolbarView breaks GraphicsOffload subsurface presentation (the video
        // rendered but presented at ~1 fps). With the video filling the split's
        // content directly, the offload subsurface presents at full rate.
        //
        // Wrap the headerbar in a box carrying the custom `.osd-header` class
        // (NOT the libadwaita built-in `.osd` class). `.osd` forces light-on-dark
        // regardless of theme — in the light theme that left the header icons
        // (incl. the hamburger menu button) light-on-light / unreadable. The
        // app-owned `apply_appearance` CSS styles `.osd-header headerbar` with
        // theme-aware colors (the `.osd-btn` header buttons get
        // `@player_icon_color`), so both light and dark themes stay readable —
        // matching the old god-object's `.osd-header` wrapper box.
        let header_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        header_box.add_css_class("osd-header");
        header_box.append(&header);
        let header_revealer = gtk::Revealer::builder()
            .valign(gtk::Align::Start)
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .reveal_child(!is_background)
            .child(&header_box)
            .build();
        if !is_background {
            main_overlay.add_overlay(&header_revealer);
            main_overlay.set_measure_overlay(&header_revealer, false);
        }

        // OSD auto-hide: reveal on pointer motion over the video, hide after
        // `OSD_HIDE_DELAY_MS` of inactivity or on pointer-leave. Mirrors the old
        // `controls/visibility.rs`. The welcome screen is up at init, so the OSD
        // never auto-hides until the first file loads.
        let trim_active = Rc::new(Cell::new(false));
        let osd = Rc::new(OsdState {
            header_revealer: header_revealer.clone(),
            controls_revealer: controls_revealer.clone(),
            main_overlay: main_overlay.clone(),
            header_menu_buttons: {
                let mut buttons = vec![open_btn.clone(), menu_btn.clone()];
                buttons.extend(controls.model().track_menu_buttons());
                buttons
            },
            controls_popovers: controls.model().popovers(),
            is_paused: Cell::new(false),
            is_welcome_visible: Cell::new(!is_background),
            trim_active: trim_active.clone(),
            hide_timeout_id: RefCell::new(None),
        });
        if is_background {
            wire_background_video_input(&main_overlay);
        } else {
            wire_osd_autohide(&main_overlay, &osd);
            // Pointer gestures over the video (mirrors the old `input/mod.rs`):
            // double-click → fullscreen, right-click → context menu, drag → move
            // the window (when not fullscreen).
            wire_video_input(&root, &main_overlay, &sender);
            // Window-level keyboard shortcuts (space/seek/volume/fullscreen/…).
            // The Relm4 port had lost this dispatch — the shortcut table was
            // display-only until this controller restored it.
            super::keyboard::wire_keyboard_shortcuts(&root, &mpv, &settings, &sender);
            // Trim-mode keys (I / O / M / Esc): a capture-phase controller that
            // only acts while trim mode is active, so it never shadows the normal
            // bindings on those keys outside trim mode.
            super::keyboard::wire_trim_keys(&root, trim_active.clone(), &sender);
        }

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
        });

        // Toast host wrapping the content (screenshot / chapter / sleep-timer
        // feedback). The split view is its child; toasts float above the video.
        // The Root IS the toast overlay (toast host wrapping the content): the
        // split view is its child; toasts float above the video.
        let toast_overlay = root.clone();
        toast_overlay.set_child(Some(&split_view));

        // Show the idle background now (welcome is up). Hidden on first
        // `FileLoaded` so the video surface becomes visible again. Kick off the
        // wallpaper detection + blur so the static gradient is replaced by the
        // blurred desktop wallpaper once it is cached.
        if !is_background {
            if let Some(display) = gtk::gdk::Display::default() {
                idle_bg.show(&display);
            }
            super::idle_wallpaper::spawn_apply(idle_bg.clone());
        }

        // mpv option strings, read once.
        let mut custom_opts = settings.get_string(keys::CUSTOM_MPV_OPTIONS).clone();
        if settings.get_bool(keys::GPU_DUMB_MODE) {
            if !custom_opts.is_empty() {
                custom_opts.push(',');
            }
            custom_opts.push_str("gpu-dumb-mode=yes");
        }
        let audio_output = settings.get_string(keys::AUDIO_OUTPUT).clone();
        let initial_volume = settings.get_double(keys::VOLUME) * 100.0;
        let muted = settings.get_bool(keys::MUTE);

        let render_ctx: RenderCtx = Rc::new(RefCell::new(None));
        let video_backend: Rc<RefCell<Option<VideoBackend>>> = Rc::new(RefCell::new(None));

        // mpv/GL comes up when the CONTENT realizes (it needs only the display +
        // overlay/gl_area, not a window — ADR D14), so it works in any mount point.
        wire_realize(
            &root,
            &main_overlay,
            &gl_area,
            &sw_picture,
            &mpv,
            &render_ctx,
            &video_backend,
            &sender,
            RealizeOpts {
                custom_opts,
                audio_output,
                initial_volume,
                muted,
            },
        );

        // Publish the shell input so the application `open` handler can deliver
        // files opened from the desktop / CLI.
        let shell_input_token = set_shell_input(sender.input_sender().clone());
        root.connect_unrealize(move |_| clear_shell_input(shell_input_token));

        // Apply the saved OSD palette + layout CSS now (the god-object did this
        // on boot) into the shared display-wide provider slot; the Appearance
        // dialog re-applies live through the same slot.
        reapply_theme(&settings);

        // Window aspect-ratio snap: resize the window to the video's aspect on
        // load and on manual resize (debounced), so no letterbox bars remain.
        // Resolves the window lazily from the content (ADR D14).
        let aspect_snap = Rc::new(AspectSnap {
            content: root.downgrade(),
            settings: settings.clone(),
            mpv: mpv.clone(),
            gl_area: gl_area.clone(),
            split_view: split_view.clone(),
            aspect: Cell::new(0.0),
            resizing: Cell::new(false),
            resize_gen: Cell::new(0),
        });

        // Window-needing wiring (close → mpv teardown, the default-size notify
        // that drives the aspect snap) is deferred to first realize, when the
        // content is actually in a window (it has none at init — the mount point
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

        ComponentParts {
            model: Self {
                mode,
                controls,
                playlist,
                iptv,
                #[cfg(feature = "chromecast")]
                cast,
                main_overlay,
                welcome_overlay: RefCell::new(welcome),
                playlist_paths: Vec::new(),
                mpv,
                duration_secs: 0.0,
                settings,
                video_transform,
                osd,
                idle_bg,
                buffering_overlay,
                toast_overlay,
                sleep_timer_source: None,
                activity_paused_playback: Cell::new(false),
                sleep_timer_remaining: 0,
                sleep_timer_action: String::new(),
                sleep_timer_volume: 100.0,
                aspect_snap,
                shell_input_token,
                trim,
                trim_progress_row,
                trim_encode,
                trim_filters: Rc::new(RefCell::new(
                    big_media_runtime::converter::profiles::VideoFilters::default(),
                )),
                trim_split: Rc::new(Cell::new(false)),
                trim_play_selection: Rc::new(Cell::new(false)),
                trim_ranges: Rc::new(RefCell::new(Vec::new())),
                trim_encode_dialog: Rc::new(RefCell::new(None)),
                trim_progress_revealer,
                trim_active,
                trim_queue: Rc::new(RefCell::new(None)),
            },
            widgets: (),
        }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        let adw_window = root.root().and_downcast::<adw::ApplicationWindow>();
        let window_ref = adw_window.as_ref();
        let gtk_window = root.root().and_downcast::<gtk::Window>();
        let gtk_window_ref = gtk_window.as_ref();
        match msg {
            VideoShellInput::Player(event) => self.on_player_event(event, window_ref),
            VideoShellInput::Controls(out) => self.on_controls(out, window_ref),
            VideoShellInput::Playlist(out) => self.on_playlist(out),
            VideoShellInput::EnterTrimMode => self.enter_trim_mode(),
            VideoShellInput::ExitTrimMode => self.exit_trim_mode(),
            VideoShellInput::TrimBar(out) => self.on_trim_bar(out, window_ref, &sender),
            VideoShellInput::TrimCancelExport => self.cancel_trim_export(),
            VideoShellInput::TrimMark => self.trim_mark(),
            VideoShellInput::TrimExportFinished(outcome) => self.finish_trim_export(outcome),
            VideoShellInput::ExtractAudio(codec) => self.extract_audio(&codec),
            VideoShellInput::QuickConvert(container) => self.quick_convert(&container),
            VideoShellInput::Iptv(out) => VideoShell::on_iptv(out, &sender),
            #[cfg(feature = "chromecast")]
            VideoShellInput::Cast(out) => self.on_cast(out),
            VideoShellInput::ShowIptvBrowser => {
                if let Some(root) = window_ref {
                    self.show_iptv_browser(root);
                }
            }
            #[cfg(feature = "chromecast")]
            VideoShellInput::ShowCastDialog => {
                if let Some(root) = window_ref {
                    self.show_cast_dialog(root);
                }
            }
            VideoShellInput::ShowFiltersDialog => {
                if let Some(root) = window_ref {
                    self.show_filters_dialog(root);
                }
            }
            VideoShellInput::ShowEqualizer => {
                if let Some(root) = window_ref {
                    self.show_equalizer_dialog(root);
                }
            }
            VideoShellInput::ShowDelay => {
                if let Some(root) = window_ref {
                    self.show_delay_dialog(root);
                }
            }
            VideoShellInput::ShowImageAdjust => {
                if let Some(root) = window_ref {
                    self.show_image_adjust_dialog(root);
                }
            }
            VideoShellInput::ShowYtdlQuality => {
                if let Some(root) = window_ref {
                    self.show_ytdl_quality_dialog(root);
                }
            }
            VideoShellInput::ShowMediaInfo => {
                if let Some(root) = window_ref {
                    self.show_media_info(root);
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
                if let Some(root) = window_ref {
                    Self::show_recent_dialog(root);
                }
            }
            VideoShellInput::ShowOpenDisc => {
                if let Some(root) = window_ref {
                    Self::show_open_disc_dialog(root);
                }
            }
            VideoShellInput::ShowPreferences => {
                if let Some(root) = window_ref {
                    self.show_preferences(root);
                }
            }
            VideoShellInput::ShowAppearance => {
                if let Some(root) = window_ref {
                    self.show_appearance(root);
                }
            }
            VideoShellInput::ShowRestoreDefaults => {
                if let Some(root) = window_ref {
                    Self::show_restore_defaults_dialog(root);
                }
            }
            VideoShellInput::RestoreDefaultsConfirmed => self.restore_all_defaults(),
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
            VideoShellInput::Screenshot => self.take_screenshot(),
            VideoShellInput::ShowChapters => {
                if let Some(root) = window_ref {
                    self.show_chapters_dialog(root, &sender);
                }
            }
            VideoShellInput::SetChapter(idx) => self.set_chapter(idx),
            VideoShellInput::ShowSleepTimer => {
                if let Some(root) = window_ref {
                    self.show_sleep_timer_dialog(root, &sender);
                }
            }
            VideoShellInput::SleepTimerStart { minutes, action } => {
                self.start_sleep_timer(sleep_minutes_to_secs(minutes), action, &sender);
            }
            VideoShellInput::SleepTimerCancel => {
                self.cancel_sleep_timer();
                self.notify(&i18n("Sleep timer cancelled"));
            }
            VideoShellInput::SleepTick => self.sleep_tick(),
            VideoShellInput::TogglePlayback => {
                if let Some(mpv) = self.mpv.borrow().as_ref() {
                    mpv.toggle_pause();
                }
            }
            VideoShellInput::ToggleFullscreen => {
                if let Some(root) = window_ref {
                    root.set_fullscreened(!root.is_fullscreen());
                }
            }
            VideoShellInput::SetPaneActivity(activity) => self.set_pane_activity(activity),
        }
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
        // Abort any in-flight clip export so a closing window does not leave an
        // orphaned ffmpeg worker running.
        self.cancel_trim_export();
        if let Some(source) = self.sleep_timer_source.take() {
            source.remove();
        }
    }
}
