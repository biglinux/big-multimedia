use super::*;

/// Shared, interior-mutable handle to the live mpv render context.
pub(super) type RenderCtx = Rc<RefCell<Option<FastRenderContext>>>;

pub(super) struct RealizeOpts {
    pub(super) custom_opts: String,
    pub(super) audio_output: String,
    pub(super) initial_volume: f64,
    pub(super) muted: bool,
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
    mpv: &Rc<RefCell<Option<Arc<MpvBackend>>>>,
    render_ctx: &RenderCtx,
    video_backend: &Rc<RefCell<Option<VideoBackend>>>,
    sender: &ComponentSender<VideoShell>,
    opts: RealizeOpts,
) {
    let main_overlay = main_overlay.clone();
    let gl_area = gl_area.clone();
    let mpv = mpv.clone();
    let render_ctx = render_ctx.clone();
    let video_backend = video_backend.clone();
    let sender = sender.clone();
    let opts = Rc::new(opts);

    realize_host.connect_realize(move |win| {
        // Realize can fire again after an unrealize; bring mpv up only once.
        if mpv.borrow().is_some() {
            return;
        }
        let display = WidgetExt::display(win);
        let backend =
            video_surface::install_best_backend(&display, &main_overlay, &gl_area, "Player");
        let is_offload = backend.is_offload();
        video_backend.replace(Some(backend));

        let (tx, rx) = async_channel::bounded::<PlayerEvent>(32);
        let mpv_backend = match MpvBackend::new(tx, &opts.custom_opts, &opts.audio_output) {
            Ok(backend) => Arc::new(backend),
            Err(e) => {
                error!("video: failed to init mpv: {e}");
                return;
            }
        };
        mpv_backend.set_volume(opts.initial_volume);
        if opts.muted {
            mpv_backend.set_mute(true);
        }
        mpv.replace(Some(mpv_backend.clone()));

        // mpv events → shell input.
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            while let Ok(event) = rx.recv().await {
                if input.send(VideoShellInput::Player(event)).is_err() {
                    break;
                }
            }
        });

        wire_render(
            &mpv_backend,
            &gl_area,
            &main_overlay,
            &render_ctx,
            &video_backend,
            is_offload,
        );
    });
}

/// Wire the mpv render path: offload (DMA-BUF/GraphicsOffload) when available,
/// GLArea fallback otherwise. Mirrors `init_render_context*` + `on_gl_render`.
fn wire_render(
    mpv: &Arc<MpvBackend>,
    gl_area: &gtk::GLArea,
    main_overlay: &gtk::Overlay,
    render_ctx: &RenderCtx,
    video_backend: &Rc<RefCell<Option<VideoBackend>>>,
    is_offload: bool,
) {
    if is_offload {
        // This callback is stored inside `render_ctx`, so it must not capture
        // `render_ctx` strongly. Otherwise the offload path keeps the render
        // context, video backend, overlay, and mpv backend alive after the host
        // window is closed.
        let on_frame = {
            let video_backend = Rc::downgrade(video_backend);
            let render_ctx = Rc::downgrade(render_ctx);
            let overlay = main_overlay.downgrade();
            move || {
                let (Some(video_backend), Some(render_ctx), Some(overlay)) = (
                    video_backend.upgrade(),
                    render_ctx.upgrade(),
                    overlay.upgrade(),
                ) else {
                    return;
                };
                let ctx_ref = render_ctx.borrow();
                let Some(ctx) = ctx_ref.as_ref() else {
                    return;
                };
                video_surface::request_offload_render(
                    video_backend.borrow().as_ref(),
                    ctx,
                    &overlay,
                );
            }
        };
        match video_surface::create_render_context(
            mpv,
            video_backend.borrow().as_ref(),
            gl_area,
            "Player",
            on_frame,
        ) {
            Ok(ctx) => {
                render_ctx.replace(Some(ctx));
            }
            Err(e) => error!("video: offload render context failed: {e}"),
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
        let render_ctx = render_ctx.clone();
        gl_area.connect_render(move |area, _ctx| {
            render_glarea_frame(&render_ctx, area);
            glib::Propagation::Stop
        });
    }
    {
        let mpv = mpv.clone();
        let render_ctx = render_ctx.clone();
        gl_area.connect_realize(move |area| ensure_glarea_ctx(&mpv, area, &render_ctx));
    }
    gl_area.connect_resize(|area, _, _| area.queue_render());
    // The GLArea may already be realized (added to a realized window); create
    // the context now so we do not miss the realize signal.
    if gl_area.is_realized() {
        ensure_glarea_ctx(mpv, gl_area, render_ctx);
    }
}

/// Create the GLArea-path render context once (GL must be current → realize).
fn ensure_glarea_ctx(mpv: &Arc<MpvBackend>, gl_area: &gtk::GLArea, render_ctx: &RenderCtx) {
    if render_ctx.borrow().is_some() {
        return;
    }
    match video_surface::create_render_context(mpv, None, gl_area, "Player GLArea", || {}) {
        Ok(ctx) => {
            render_ctx.replace(Some(ctx));
        }
        Err(e) => error!("video: GLArea render context failed: {e}"),
    }
}

/// Render one mpv frame into the GLArea's framebuffer (fallback path).
fn render_glarea_frame(render_ctx: &RenderCtx, gl_area: &gtk::GLArea) {
    let ctx_ref = render_ctx.borrow();
    let Some(ctx) = ctx_ref.as_ref() else {
        return;
    };
    if skip_mpv_render() {
        ctx.report_swap();
        return;
    }
    if !ctx.has_new_frame() {
        return;
    }
    gl_area.attach_buffers();
    let mut fbo: i32 = 0;
    if let Some(get_integerv) = get_gl_get_integerv() {
        // SAFETY: the cached proc matches `glGetIntegerv`; GL_FRAMEBUFFER_BINDING
        // writes a single i32.
        unsafe { get_integerv(GL_FRAMEBUFFER_BINDING, &raw mut fbo) };
    }
    let scale = gl_area.scale_factor();
    video_surface::render_glarea_frame(
        ctx,
        fbo,
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
