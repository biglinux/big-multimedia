use super::*;

/// Shared, interior-mutable handle to the live mpv render context.
pub(super) type FastRenderContextSlot = Rc<RefCell<Option<FastRenderContext>>>;

/// Interior-mutable handle to the software (no-GL) render context.
type SoftwareRenderContextSlot = Rc<RefCell<Option<SoftwareRenderContext>>>;

pub(super) struct VideoRealizeOptions {
    pub(super) custom_mpv_options: String,
    pub(super) audio_output: String,
    pub(super) initial_volume: f64,
    pub(super) muted: bool,
    /// Video output mode (settings `render-mode`): `"auto"` (default — best GL
    /// backend + software fallback on failure), `"gl"` (force GL/offload), `"sw"`
    /// (force the software/no-GL render path).
    pub(super) render_mode: String,
}

/// On content realize, pick the best video backend, bring up mpv, and wire the
/// render path. Mirrors the god-object's `setup_realize_handler` + `init_mpv`.
///
/// `realize_host` is the mountable content (ADR D14), NOT a window: the GL
/// backend needs only a `gdk::Display` (per-screen, identical for any widget) +
/// the overlay/gl_area, so mpv comes up whenever the content realizes in
/// whatever window hosts it.
#[allow(clippy::too_many_arguments)]
pub(super) fn wire_realize(
    realize_host: &impl IsA<gtk::Widget>,
    main_overlay: &gtk::Overlay,
    gl_area: &gtk::GLArea,
    software_picture: &gtk::Picture,
    mpv: &Rc<RefCell<Option<Arc<MpvBackend>>>>,
    fast_render_context_slot: &FastRenderContextSlot,
    video_backend: &Rc<RefCell<Option<VideoBackend>>>,
    sender: &ComponentSender<VideoShell>,
    realize_options: VideoRealizeOptions,
) {
    let main_overlay = main_overlay.clone();
    let gl_area = gl_area.clone();
    let software_picture = software_picture.clone();
    let software_render_context_slot: SoftwareRenderContextSlot = Rc::new(RefCell::new(None));
    let mpv = mpv.clone();
    let fast_render_context_slot = fast_render_context_slot.clone();
    let video_backend = video_backend.clone();
    let sender = sender.clone();
    let realize_options = Rc::new(realize_options);

    realize_host.connect_realize(move |realized_widget| {
        // Realize can fire again after an unrealize; bring mpv up only once.
        if mpv.borrow().is_some() {
            return;
        }
        let (player_event_sender, player_event_receiver) =
            async_channel::bounded::<PlayerEvent>(32);
        let mpv_backend = match MpvBackend::new(
            player_event_sender,
            &realize_options.custom_mpv_options,
            &realize_options.audio_output,
        ) {
            Ok(backend) => Arc::new(backend),
            Err(e) => {
                error!("video: failed to init mpv: {e}");
                return;
            }
        };
        mpv_backend.set_volume(realize_options.initial_volume);
        if realize_options.muted {
            mpv_backend.set_mute(true);
        }
        mpv.replace(Some(mpv_backend.clone()));

        // mpv events → shell input.
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = player_event_receiver.recv().await {
                if input.send(VideoShellInput::Player(event)).is_err() {
                    break;
                }
            }
        });

        // Render path by mode (settings `render-mode`, default "auto"). "sw"
        // forces the software/no-GL path directly — never touching GL/offload —
        // so video plays where GL is not viable (e.g. a GL-less VM). "gl"/"auto"
        // install the best GL backend; the GL paths fall back to software on a
        // render-context error (see `wire_render`).
        let render_mode = match realize_options.render_mode.as_str() {
            "" => "auto",
            other => other,
        };
        if render_mode == "sw" {
            log::info!("video: render-mode=sw — forcing the software render path");
            video_backend.replace(None);
            wire_software_fallback(
                &mpv_backend,
                &gl_area,
                &software_picture,
                &software_render_context_slot,
            );
            return;
        }
        let display = WidgetExt::display(realized_widget);
        let selected_video_backend =
            video_surface::install_best_backend(&display, &main_overlay, &gl_area, "Player");
        let is_offload_backend = selected_video_backend.is_offload();
        video_backend.replace(Some(selected_video_backend));
        wire_render(
            &mpv_backend,
            &gl_area,
            &software_picture,
            &main_overlay,
            &fast_render_context_slot,
            &software_render_context_slot,
            &video_backend,
            is_offload_backend,
        );
    });
}

/// Wire the mpv render path: offload (DMA-BUF/GraphicsOffload) when available,
/// GLArea fallback otherwise. Mirrors `init_render_context*` + `on_gl_render`.
#[allow(clippy::too_many_arguments)]
fn wire_render(
    mpv: &Arc<MpvBackend>,
    gl_area: &gtk::GLArea,
    software_picture: &gtk::Picture,
    main_overlay: &gtk::Overlay,
    fast_render_context_slot: &FastRenderContextSlot,
    software_render_context_slot: &SoftwareRenderContextSlot,
    video_backend: &Rc<RefCell<Option<VideoBackend>>>,
    is_offload_backend: bool,
) {
    if is_offload_backend {
        // This callback is stored inside `fast_render_context_slot`, so it must not capture
        // `fast_render_context_slot` strongly. Otherwise the offload path keeps the render
        // context, video backend, overlay, and mpv backend alive after the host
        // window is closed.
        let render_frame_callback = {
            let video_backend = Rc::downgrade(video_backend);
            let fast_render_context_slot = Rc::downgrade(fast_render_context_slot);
            let overlay = main_overlay.downgrade();
            move || {
                let (Some(video_backend), Some(fast_render_context_slot), Some(overlay)) = (
                    video_backend.upgrade(),
                    fast_render_context_slot.upgrade(),
                    overlay.upgrade(),
                ) else {
                    return;
                };
                let fast_context_borrow = fast_render_context_slot.borrow();
                let Some(fast_render_context) = fast_context_borrow.as_ref() else {
                    return;
                };
                video_surface::request_offload_render(
                    video_backend.borrow().as_ref(),
                    fast_render_context,
                    &overlay,
                );
            }
        };
        match video_surface::create_render_context(
            mpv,
            video_backend.borrow().as_ref(),
            gl_area,
            "Player",
            render_frame_callback,
        ) {
            Ok(fast_render_context) => {
                fast_render_context_slot.replace(Some(fast_render_context));
            }
            Err(e) => {
                error!("video: offload render context failed: {e}; trying software fallback");
                wire_software_fallback(
                    mpv,
                    gl_area,
                    software_picture,
                    software_render_context_slot,
                );
            }
        }
        // `create_render_context` makes the offload renderer's GL context current
        // to build the mpv render context but does not release it; leaving it
        // current on the main thread collides with GTK's own GSK GL renderer
        // (frozen UI + no video, while audio keeps playing on its own thread).
        // The god-object cleared it explicitly here — do the same.
        gtk::gdk::GLContext::clear_current();
        return;
    }

    // GLArea fallback: drive the frame loop manually.
    gl_area.set_auto_render(false);
    gl_area.set_has_depth_buffer(false);
    gl_area.set_has_stencil_buffer(false);

    {
        // Handler lives on the GLArea and captures only render state; no
        // ancestor widget capture, so closing the content can tear the tree down.
        let fast_render_context_slot = fast_render_context_slot.clone();
        gl_area.connect_render(move |area, _context| {
            render_glarea_frame(&fast_render_context_slot, area);
            glib::Propagation::Stop
        });
    }
    {
        let mpv = mpv.clone();
        let fast_render_context_slot = fast_render_context_slot.clone();
        let software_picture = software_picture.clone();
        let software_render_context_slot = software_render_context_slot.clone();
        gl_area.connect_realize(move |area| {
            ensure_glarea_context(
                &mpv,
                area,
                &software_picture,
                &fast_render_context_slot,
                &software_render_context_slot,
            );
        });
    }
    gl_area.connect_resize(|area, _, _| area.queue_render());
    // The GLArea may already be realized (added to a realized window); create
    // the context now so we do not miss the realize signal.
    if gl_area.is_realized() {
        ensure_glarea_context(
            mpv,
            gl_area,
            software_picture,
            fast_render_context_slot,
            software_render_context_slot,
        );
    }
}

/// Create the GLArea-path render context once (GL must be current → realize).
/// When GL is unavailable the context creation fails — fall back to the
/// software render path so video still plays on no-GL hosts.
fn ensure_glarea_context(
    mpv: &Arc<MpvBackend>,
    gl_area: &gtk::GLArea,
    software_picture: &gtk::Picture,
    fast_render_context_slot: &FastRenderContextSlot,
    software_render_context_slot: &SoftwareRenderContextSlot,
) {
    if fast_render_context_slot.borrow().is_some()
        || software_render_context_slot.borrow().is_some()
    {
        return;
    }
    match video_surface::create_render_context(mpv, None, gl_area, "Player GLArea", || {}) {
        Ok(fast_render_context) => {
            fast_render_context_slot.replace(Some(fast_render_context));
        }
        Err(e) => {
            error!("video: GLArea render context failed: {e}; using software fallback");
            wire_software_fallback(mpv, gl_area, software_picture, software_render_context_slot);
        }
    }
}

/// Software (no-GL) video path: mpv renders each frame into a CPU buffer that we
/// publish as a `gdk::MemoryTexture` on `software_picture`. Driven by a ~60 fps
/// main-thread timer gated on `has_new_frame` (idle when paused/static). Swaps
/// the dead GLArea out for the picture. Used only when the GL render context
/// could not be created.
fn wire_software_fallback(
    mpv: &Arc<MpvBackend>,
    gl_area: &gtk::GLArea,
    software_picture: &gtk::Picture,
    software_render_context_slot: &SoftwareRenderContextSlot,
) {
    if software_render_context_slot.borrow().is_some() {
        return;
    }
    let software_render_context = match mpv.create_sw_render_context() {
        Ok(software_render_context) => software_render_context,
        Err(e) => {
            error!("video: software render context failed: {e}");
            return;
        }
    };
    software_render_context_slot.replace(Some(software_render_context));
    gl_area.set_visible(false);
    software_picture.set_visible(true);
    log::info!("video: software render fallback active (no OpenGL)");

    let software_frame_buffer: Rc<RefCell<Vec<u8>>> = Rc::new(RefCell::new(Vec::new()));
    let software_render_context_slot = software_render_context_slot.clone();
    let picture_weak = software_picture.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
        let Some(picture) = picture_weak.upgrade() else {
            return glib::ControlFlow::Break; // widget gone → stop + drop the context clone
        };
        let software_context_borrow = software_render_context_slot.borrow();
        let Some(software_render_context) = software_context_borrow.as_ref() else {
            return glib::ControlFlow::Break;
        };
        if software_render_context.has_new_frame() {
            render_software_frame(&picture, software_render_context, &software_frame_buffer);
        }
        glib::ControlFlow::Continue
    });
}

/// Render one mpv frame into `buffer` (bgr0) and publish it on `picture`.
fn render_software_frame(
    picture: &gtk::Picture,
    software_render_context: &SoftwareRenderContext,
    software_frame_buffer: &Rc<RefCell<Vec<u8>>>,
) {
    let scale = picture.scale_factor().max(1);
    let width = picture.width() * scale;
    let height = picture.height() * scale;
    if width <= 0 || height <= 0 {
        return; // not allocated yet — the next tick retries
    }
    let stride = width as usize * 4;
    let needed = stride * height as usize;
    let mut frame_buffer_borrow = software_frame_buffer.borrow_mut();
    if frame_buffer_borrow.len() != needed {
        frame_buffer_borrow.resize(needed, 0);
    }
    if software_render_context.render_into(&mut frame_buffer_borrow, width, height, stride) < 0 {
        return;
    }
    let bytes = gtk::glib::Bytes::from(&frame_buffer_borrow[..]);
    let texture =
        gdk::MemoryTexture::new(width, height, gdk::MemoryFormat::B8g8r8x8, &bytes, stride);
    picture.set_paintable(Some(&texture));
}

/// Render one mpv frame into the GLArea's framebuffer (fallback path).
fn render_glarea_frame(fast_render_context_slot: &FastRenderContextSlot, gl_area: &gtk::GLArea) {
    let fast_context_borrow = fast_render_context_slot.borrow();
    let Some(fast_render_context) = fast_context_borrow.as_ref() else {
        return;
    };
    if skip_mpv_render() {
        fast_render_context.report_swap();
        return;
    }
    if !fast_render_context.has_new_frame() {
        return;
    }
    gl_area.attach_buffers();
    let mut framebuffer_binding: i32 = 0;
    if let Some(get_integerv) = get_gl_get_integerv() {
        // SAFETY: the cached proc matches `glGetIntegerv`; GL_FRAMEBUFFER_BINDING
        // writes a single i32.
        unsafe { get_integerv(GL_FRAMEBUFFER_BINDING, &raw mut framebuffer_binding) };
    }
    let scale = gl_area.scale_factor();
    video_surface::render_glarea_frame(
        fast_render_context,
        framebuffer_binding,
        gl_area.width() * scale,
        gl_area.height() * scale,
        true,
    );
}

/// On close, flush settings and shut mpv's audio output down cleanly.
pub(super) fn wire_close(
    window: &adw::ApplicationWindow,
    mpv: &Rc<RefCell<Option<Arc<MpvBackend>>>>,
    settings: &Settings,
) {
    let mpv = mpv.clone();
    let settings = settings.clone();
    window.connect_close_request(move |_| {
        if let Some(mpv) = mpv.borrow().as_ref() {
            mpv.shutdown_audio();
        }
        settings.flush();
        glib::Propagation::Proceed
    });
}
