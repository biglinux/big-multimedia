//! Heaptrack workload: pass 5 and 25 cycles in two isolated runs and compare
//! retained allocations. Use `heaptrack --record-only` to avoid opening its GUI.

use gtk::prelude::*;
use relm4::gtk;

fn main() {
    gtk::init().unwrap();
    let cycles: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    for _ in 0..cycles {
        let button = gtk::Button::new();
        let label = gtk::Label::new(Some("Name"));
        big_relm4_components::accessible::relate(
            &button,
            gtk::AccessibleRelation::LabelledBy,
            &[label.upcast_ref()],
        );
    }
}
