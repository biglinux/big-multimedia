//! The trim-mode cut timeline as a slider over the playback position.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use crate::time::playback_time;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct CutTimeline;

    #[glib::object_subclass]
    impl ObjectSubclass for CutTimeline {
        const NAME: &'static str = "BigMediaCutTimeline";
        type Type = super::CutTimeline;
        type ParentType = gtk::DrawingArea;
        // GTK exposes the AT-SPI Value interface only on an AccessibleRange.
        type Interfaces = (gtk::AccessibleRange,);

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Slider);
        }
    }

    impl ObjectImpl for CutTimeline {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj()
                .connect_has_focus_notify(|timeline| timeline.queue_draw());
        }
    }

    impl WidgetImpl for CutTimeline {
        // Theme CSS draws no focus ring on a drawing area.
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.parent_snapshot(snapshot);
            let timeline = self.obj();
            if timeline.has_visible_focus() {
                let bounds = gtk::graphene::Rect::new(
                    0.0,
                    0.0,
                    timeline.width() as f32,
                    timeline.height() as f32,
                );
                snapshot.append_border(
                    &gtk::gsk::RoundedRect::from_rect(bounds, 6.0),
                    &[2.0; 4],
                    &[timeline.color(); 4],
                );
            }
        }
    }
    impl DrawingAreaImpl for CutTimeline {}
    impl AccessibleImpl for CutTimeline {}
    impl AccessibleRangeImpl for CutTimeline {}
}

glib::wrapper! {
    /// Drawing area of the trim-mode timeline, focusable and exposed to
    /// assistive technology as a slider whose value is the playback position
    /// in seconds.
    pub struct CutTimeline(ObjectSubclass<imp::CutTimeline>)
        @extends gtk::DrawingArea, gtk::Widget,
        @implements gtk::Accessible, gtk::AccessibleRange, gtk::Buildable, gtk::ConstraintTarget;
}

impl CutTimeline {
    /// Build an empty timeline named `label`.
    #[must_use]
    pub fn new(label: &str) -> Self {
        let timeline: Self = glib::Object::builder()
            .property("hexpand", true)
            .property("height-request", 20)
            .property("focusable", true)
            .build();
        timeline.update_property(&[
            gtk::accessible::Property::Label(label),
            gtk::accessible::Property::ValueMin(0.0),
        ]);
        timeline.set_position(0.0, 0.0);
        timeline
    }

    /// Expose the playhead at `position_secs` of `duration_secs`; the value
    /// text reads the position as a time.
    pub fn set_position(&self, position_secs: f64, duration_secs: f64) {
        let duration = duration_secs.max(0.0);
        let position = position_secs.clamp(0.0, duration);
        self.update_property(&[
            gtk::accessible::Property::ValueMax(duration),
            gtk::accessible::Property::ValueNow(position),
            gtk::accessible::Property::ValueText(&playback_time(position)),
        ]);
    }
}
