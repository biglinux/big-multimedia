use big_player_ui::cut_timeline::wire_cut_timeline_interaction;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[test]
#[ignore = "requires an isolated GTK session"]
fn timeline_finalizes_with_its_controllers() {
    gtk::init().expect("isolated GTK session");
    struct Finalized(Rc<Cell<usize>>);
    impl Drop for Finalized {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for size in [0, 5, 25] {
        let finalized = Rc::new(Cell::new(0));
        for _ in 0..25 {
            let area = gtk::DrawingArea::new();
            // SAFETY: this test owns the key and never reads or replaces its data.
            // Qdata is released at finalization, unlike dispose notifications.
            unsafe {
                area.set_qdata(
                    gtk::glib::Quark::from_str("timeline-finalization"),
                    Finalized(finalized.clone()),
                );
            }
            let segments = Rc::new(RefCell::new(
                (0..size)
                    .map(|id| (id, id as f64, id as f64 + 0.5))
                    .collect(),
            ));
            wire_cut_timeline_interaction(
                &area,
                &segments,
                &Rc::new(Cell::new(100.0)),
                |_| {},
                |_| {},
            );
            drop(area);
            assert_eq!(
                Rc::strong_count(&segments),
                1,
                "retained segments, size={size}"
            );
        }
        assert_eq!(finalized.get(), 25, "timeline finalizations, size={size}");
    }
}
