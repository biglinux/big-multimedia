//! Progress page callback wiring — back/cancel-all/cancel-conversion closures.

use super::ProgressPage;

impl ProgressPage {
    /// Borrow the root [`gtk::Box`] for embedding.
    pub fn widget(&self) -> &gtk::Box {
        &self.container
    }

    /// Install the callback fired when the user clicks "Back" after completion.
    pub fn connect_back<F: Fn() + 'static>(&self, f: F) {
        *self.on_back.borrow_mut() = Some(Box::new(f));
    }

    /// Install the callback fired when the user clicks "Cancel all".
    pub fn connect_cancel_all<F: Fn() + 'static>(&self, f: F) {
        *self.on_cancel_all.borrow_mut() = Some(Box::new(f));
    }

    /// Install the callback fired when the user cancels one conversion row.
    pub fn connect_cancel_conversion<F: Fn(u64) + 'static>(&self, f: F) {
        *self.on_cancel_conversion.borrow_mut() = Some(Box::new(f));
    }
}
