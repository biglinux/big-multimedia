use super::*;
use crate::player::thumbnail::{THUMB_HEIGHT, THUMB_WIDTH};
use crate::settings::keys;
use big_media_components::time::playback_time;
use gtk::{gdk, glib};

/// Build the hover-preview popover: a thumbnail frame above a time (+ chapter)
/// label, parented to the seek bar. Non-autohide so it never grabs input; the
/// hover controller pops it up / down.
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
/// hides it.
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
            if show_thumb && thumb_gen.request(file.as_deref().unwrap_or_default(), time) {
                thumb.set_paintable(gdk::Paintable::NONE);
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

/// Keep the latest requested preview ready, including while the pointer is away.
pub(super) fn wire_thumb_consumer(
    thumb_rx: async_channel::Receiver<ThumbnailResult>,
    thumb: &gtk::Picture,
    thumb_gen: &Rc<ThumbnailGenerator>,
) {
    // Weak: this main-context future outlives the bar (it ends only when
    // the worker drops the channel) — strong captures pinned the preview
    // popover (and its subtree) after window close.
    let thumb = thumb.downgrade();
    let thumb_gen = Rc::downgrade(thumb_gen);
    glib::spawn_future_local(async move {
        while let Ok(result) = thumb_rx.recv().await {
            let (Some(thumb), Some(generator)) = (thumb.upgrade(), thumb_gen.upgrade()) else {
                break;
            };
            if !generator.is_current(&result) {
                continue;
            }
            let bytes = glib::Bytes::from_owned(result.rgb_data);
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

/// Hovering Previous (`step` -1) or Next (+1) shows where it goes: that
/// video's picture and name, in the order the playlist plays (shuffled
/// too). Nothing shows where there is nowhere to go.
pub(super) fn wire_neighbour_preview(button: &gtk::Button, mpv: &MpvSlot, step: i64) {
    // bigagents: popover-parent-managed — one reused popover per button, for
    // the life of the controls bar.
    let popover = gtk::Popover::builder()
        .autohide(false)
        .position(gtk::PositionType::Top)
        .css_classes(["seek-preview"])
        .build();
    popover.set_parent(button);
    {
        let popover = popover.downgrade();
        button.connect_destroy(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
                if popover.parent().is_some() {
                    popover.unparent();
                }
            }
        });
    }
    let thumb = gtk::Picture::builder()
        .width_request(THUMB_WIDTH)
        .height_request(THUMB_HEIGHT)
        .content_fit(gtk::ContentFit::Contain)
        .css_classes(["seek-preview-thumb"])
        .build();
    let heading = gtk::Label::builder()
        .label(if step < 0 {
            i18n("Previous")
        } else {
            i18n("Next")
        })
        .css_classes(["caption", "dim-label"])
        .build();
    let name = gtk::Label::builder()
        .css_classes(["seek-preview-time"])
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(30)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.append(&thumb);
    content.append(&heading);
    content.append(&name);
    popover.set_child(Some(&content));

    // The video whose picture is up (or on its way).
    let shown: Rc<RefCell<Option<String>>> = Rc::default();
    let motion = gtk::EventControllerMotion::new();
    {
        let mpv = mpv.clone();
        let popover = popover.clone();
        motion.connect_enter(move |_, _, _| {
            let Some(path) = mpv.borrow().as_ref().and_then(|mpv| {
                let index = usize::try_from(mpv.get_playlist_pos() + step).ok()?;
                mpv.get_playlist_paths().into_iter().nth(index)
            }) else {
                return;
            };
            name.set_label(&display_name(&path));
            if shown.borrow().as_deref() != Some(path.as_str()) {
                shown.replace(Some(path.clone()));
                thumb.set_paintable(gdk::Paintable::NONE);
                thumb.set_visible(!is_stream_uri(&path));
                if !is_stream_uri(&path) {
                    let thumb = thumb.downgrade();
                    let shown = shown.clone();
                    glib::spawn_future_local(async move {
                        let wanted = path.clone();
                        let picture = gtk::gio::spawn_blocking(move || {
                            crate::media::thumbnail::get_or_generate_thumbnail(&wanted)
                        })
                        .await
                        .ok()
                        .flatten();
                        let (Some(picture), Some(thumb)) = (picture, thumb.upgrade()) else {
                            return;
                        };
                        // Still the one hovered: the pointer may have moved on.
                        if shown.borrow().as_deref() == Some(path.as_str())
                            && let Ok(texture) = gdk::Texture::from_filename(&picture)
                        {
                            thumb.set_paintable(Some(&texture));
                        }
                    });
                }
            }
            popover.popup();
        });
    }
    motion.connect_leave(move |_| popover.popdown());
    button.add_controller(motion);
}

/// A playlist entry as a person names it: the file name without its
/// extension, or a stream's last path segment.
fn display_name(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or(trimmed);
    std::path::Path::new(last).file_stem().map_or_else(
        || last.to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    )
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
