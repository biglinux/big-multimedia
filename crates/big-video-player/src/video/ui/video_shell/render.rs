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
// Unrelated GTK widgets, render-context slots, sender, and options — no
// cohesive group to fold into a param struct without hiding the wiring.
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
                if let Some(VideoBackend::Offload(renderer)) = video_backend.borrow().as_ref() {
                    renderer.widget().set_visible(false);
                }
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

/// Compatibility renderer: wake on mpv updates, buffer release or resize, not
/// on a permanent 16 ms timer. CPU pixels are leased to GBytes without a copy.
fn wire_software_fallback(
    mpv: &Arc<MpvBackend>,
    gl_area: &gtk::GLArea,
    software_picture: &gtk::Picture,
    software_render_context_slot: &SoftwareRenderContextSlot,
) {
    if software_render_context_slot.borrow().is_some() { return; }
    let mut context = match mpv.create_sw_render_context() {
        Ok(context) => context,
        Err(error) => {
            error!("video: software render context failed: {error}");
            show_render_error(software_picture,
                &i18n("Video could not start. Try Automatic video output in Preferences, or check that mpv is installed."));
            return;
        }
    };
    let (wake, receiver) = async_channel::bounded::<()>(1);
    let wake = Arc::new(wake);
    let callback_wake = wake.clone();
    context.set_update_callback(move || { let _ = callback_wake.try_send(()); });
    software_render_context_slot.replace(Some(context));
    gl_area.set_visible(false);
    software_picture.set_visible(true);

    // Picture has no resize signal. A transparent DrawingArea in the same
    // overlay observes allocation changes without polling or painting pixels.
    if let Some(overlay) = software_picture.parent().and_downcast::<gtk::Overlay>() {
        let probe = gtk::DrawingArea::new();
        probe.set_can_target(false);
        probe.set_opacity(0.0);
        probe.set_hexpand(true);
        probe.set_vexpand(true);
        let resize_wake = wake.clone();
        probe.connect_resize(move |_, _, _| { let _ = resize_wake.try_send(()); });
        overlay.add_overlay(&probe);
    }
    let weak_wake = Arc::downgrade(&wake);
    let slot = Rc::downgrade(software_render_context_slot);
    let picture = software_picture.downgrade();
    let pool = crate::player::mpv_backend::software_frames::SoftwareFramePool::default();
    glib::spawn_future_local(async move {
        let mut pending = true;
        let mut last_size = (0, 0, 0);
        let mut error_shown = false;
        while receiver.recv().await.is_ok() {
            let (Some(slot), Some(picture), Some(wake)) = (slot.upgrade(), picture.upgrade(), weak_wake.upgrade()) else { break };
            let borrowed = slot.borrow();
            let Some(context) = borrowed.as_ref() else { break };
            let size = (picture.width(), picture.height(), picture.scale_factor());
            pending |= context.has_new_frame() || size != last_size;
            if !pending { continue; }
            match render_software_frame(&picture, context, &pool, wake) {
                Ok(true) => { pending = false; last_size = size; error_shown = false; }
                Ok(false) => {} // all buffers retained or not yet allocated
                Err(message) => {
                    pending = false;
                    if !error_shown { show_render_error(&picture, &message); error_shown = true; }
                }
            }
        }
    });
}

fn show_render_error(picture: &gtk::Picture, message: &str) {
    if let Some(overlay) = picture.ancestor(adw::ToastOverlay::static_type()).and_downcast::<adw::ToastOverlay>() {
        let toast = adw::Toast::new(message);
        toast.set_timeout(0); // do not disappear before the user can read it
        overlay.add_toast(toast);
    }
}

fn render_software_frame(
    picture: &gtk::Picture,
    context: &SoftwareRenderContext,
    pool: &crate::player::mpv_backend::software_frames::SoftwareFramePool,
    wake: Arc<async_channel::Sender<()>>,
) -> std::result::Result<bool, String> {
    use crate::player::mpv_backend::frame_layout::aligned_layout;
    let scale = picture.scale_factor().max(1);
    let width = picture.width().checked_mul(scale).unwrap_or(0);
    let height = picture.height().checked_mul(scale).unwrap_or(0);
    if width <= 0 || height <= 0 { return Ok(false); }
    let message = || i18n("The video area is too large for compatibility mode. Reduce the window size or select Automatic video output in Preferences.");
    let (stride, length) = aligned_layout(width, height).ok_or_else(message)?;
    let Some(mut frame) = pool.acquire(length, wake).map_err(|_| message())? else { return Ok(false) };
    if context.render_into(frame.pixels_mut(), width, height, stride) < 0 {
        return Err(i18n("The video frame could not be displayed. Try Automatic video output in Preferences."));
    }
    let bytes = glib::Bytes::from_owned(frame);
    let texture = gdk::MemoryTexture::new(width, height, gdk::MemoryFormat::B8g8r8x8, &bytes, stride);
    picture.set_paintable(Some(&texture));
    Ok(true)
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
    // A GTK resize/expose request must redraw even while mpv is paused.
    // Still service update() with the area's GL context current.
    let _ = fast_render_context.has_new_frame();
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
        gl_area.width().checked_mul(scale).unwrap_or(0),
        gl_area.height().checked_mul(scale).unwrap_or(0),
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
