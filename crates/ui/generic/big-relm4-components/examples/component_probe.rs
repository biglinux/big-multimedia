//! Heaptrack workload for repeatedly starting and stopping Relm4 components.
use relm4::gtk;
use relm4::{Component, ComponentParts, ComponentSender, SimpleComponent};

struct Probe;
impl SimpleComponent for Probe {
    type Init = ();
    type Input = ();
    type Output = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(_: (), _: Self::Root, _: ComponentSender<Self>) -> ComponentParts<Self> {
        ComponentParts {
            model: Self,
            widgets: (),
        }
    }

    fn update(&mut self, _: (), _: ComponentSender<Self>) {}
}

fn main() {
    gtk::init().unwrap();
    let cycles: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let context = gtk::glib::MainContext::default();
    for _ in 0..cycles {
        let controller = Probe::builder().launch(()).detach();
        drop(controller);
        while context.pending() {
            context.iteration(false);
        }
    }
}
