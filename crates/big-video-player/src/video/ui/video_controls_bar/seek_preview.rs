use super::*;
use crate::player::thumbnail::{THUMB_HEIGHT, THUMB_WIDTH};
use crate::settings::keys;
use big_media_components::time::playback_time;
use gtk::{gdk, glib};

/// Build the hover-preview popover: a thumbnail frame above a time (+ chapter)
/// label, parented to the seek bar. Non-autohide so it never grabs input; the
/// hover controller pops it up / down. Mirrors the old `seek_tooltip_box`.
pub(super) fn build_seek_preview(anchor: &gtk::Widget) -> (gtk::Popover, gtk::Picture, gtk::Label) {
    // bigagents: popover-parent-managed — single reused seek-preview popover,
    // lives exactly as long as the controls bar (app lifetime). Parented to the
    // shared bar-stack so it works for both the seek scale and the cut timeline.
    let popover = gtk::Popover::builder()
        .autohide(false)
        .position(gtk::PositionType::Top)
        .css_classes(["seek-preview"])
        .build();
    popover.set_parent(anchor);
    {
        // The anchor does not unparent foreign children at dispose (it
        // finalized "with children left" and the popover dangled).
        let popover = popover.downgrade();
        anchor.connect_destroy(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
                if popover.parent().is_some() {
                    popover.unparent();
                }
            }
        });
    }

    let preview_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let thumb = gtk::Picture::builder()
        .width_request(THUMB_WIDTH)
        .height_request(THUMB_HEIGHT)
        .css_classes(["seek-preview-thumb"])
        .build();
    thumb.update_property(&[gtk::accessible::Property::Label(&i18n("Seek preview"))]);
    let label = gtk::Label::builder()
        .css_classes(["seek-preview-time"])
        .justify(gtk::Justification::Center)
        .build();
    preview_box.append(&thumb);
    preview_box.append(&label);
    popover.set_child(Some(&preview_box));
    (popover, thumb, label)
}

/// Wire the seek-bar hover preview: pointer motion over the bar shows the time
/// (+ chapter name) at that position and requests a thumbnail frame; leave
/// hides it. Mirrors the old `update_seek_hover_label`.
// Nine unrelated GTK handles + shared state cells; bundling them into a struct
// would only rename the fan-out, not reduce it.
#[allow(clippy::too_many_arguments)]
pub(super) fn wire_seek_hover(
    anchor: &gtk::Widget,
    popover: &gtk::Popover,
    thumb: &gtk::Picture,
    label: &gtk::Label,
    duration_secs: &Rc<Cell<f64>>,
    chapters: &Rc<RefCell<Vec<(f64, String)>>>,
    current_file: &Arc<Mutex<Option<String>>>,
    thumb_gen: &Rc<ThumbnailGenerator>,
    settings: &Settings,
) {
    let motion = gtk::EventControllerMotion::new();
    let update = {
        // Weak: this closure is held by controllers ON the anchor — a strong
        // self capture pinned the controls bar subtree after close.
        let anchor = anchor.downgrade();
        let popover = popover.clone();
        let thumb = thumb.clone();
        let label = label.clone();
        let duration_secs = duration_secs.clone();
        let chapters = chapters.clone();
        let current_file = current_file.clone();
        let thumb_gen = thumb_gen.clone();
        let settings = settings.clone();
        move |x: f64| {
            let Some(anchor) = anchor.upgrade() else {
                return;
            };
            let duration = duration_secs.get();
            let width = f64::from(anchor.width());
            if duration <= 0.0 || width <= 0.0 {
                return;
            }
            let time = (x / width).clamp(0.0, 1.0) * duration;

            let mut text = playback_time(time);
            if let Some(name) = chapter_name_at(&chapters.borrow(), time) {
                text.push('\n');
                text.push_str(name);
            }
            label.set_label(&text);

            // Streams: ffmpeg seek is slow → skip the thumb to stay responsive.
            let file = current_file
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let show_thumb = settings.get_bool(keys::SEEKBAR_THUMBNAIL)
                && file.as_deref().is_some_and(|f| !is_stream_uri(f));
            thumb.set_visible(show_thumb);
            if show_thumb {
                thumb_gen.request("", time);
            }

            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, 0, 1, 1)));
            popover.popup();
        }
    };
    {
        let update = update.clone();
        motion.connect_enter(move |_, x, _| update(x));
    }
    motion.connect_motion(move |_, x, _| update(x));
    {
        let popover = popover.clone();
        motion.connect_leave(move |_| popover.popdown());
    }
    anchor.add_controller(motion);
}

/// Drain thumbnail results and paint them into the preview while it is shown.
/// Mirrors the old `setup_thumbnail_generator` consumer.
pub(super) fn wire_thumb_consumer(
    thumb_rx: async_channel::Receiver<ThumbnailResult>,
    popover: &gtk::Popover,
    thumb: &gtk::Picture,
) {
    // Weak: this main-context future outlives the bar (it ends only when
    // the worker drops the channel) — strong captures pinned the preview
    // popover (and its subtree) after window close.
    let popover = popover.downgrade();
    let thumb = thumb.downgrade();
    glib::spawn_future_local(async move {
        while let Ok(result) = thumb_rx.recv().await {
            let (Some(popover), Some(thumb)) = (popover.upgrade(), thumb.upgrade()) else {
                break;
            };
            if !popover.is_visible() {
                continue;
            }
            let bytes = glib::Bytes::from(&result.rgb_data);
            let texture = gdk::MemoryTexture::new(
                THUMB_WIDTH,
                THUMB_HEIGHT,
                gdk::MemoryFormat::R8g8b8,
                &bytes,
                (THUMB_WIDTH * 3) as usize,
            );
            thumb.set_paintable(Some(&texture));
        }
    });
}

/// Title of the last chapter at or before `time` (non-empty), else `None`.
fn chapter_name_at(chapters: &[(f64, String)], time: f64) -> Option<&str> {
    let mut current: Option<&str> = None;
    for (start, title) in chapters {
        if *start <= time {
            current = Some(title.as_str());
        } else {
            break;
        }
    }
    current.filter(|title| !title.is_empty())
}

/// True for a remote / stream URI whose ffmpeg seek is too slow to thumbnail.
fn is_stream_uri(path: &str) -> bool {
    path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("rtsp://")
        || path.starts_with("rtmp://")
}
