//! Password entry rows whose private buffer is released with the row.

use adw::prelude::*;

/// Build a row and release libadwaita 1.9's leaked buffer creation reference.
pub fn password_row(builder: adw::builders::PasswordEntryRowBuilder) -> adw::PasswordEntryRow {
    #[allow(clippy::disallowed_methods)]
    let row = builder.build();
    let buffer = row
        .delegate()
        .and_then(|delegate| delegate.downcast::<gtk::Text>().ok())
        .map(|text| text.buffer());
    // GtkText, this local handle, and libadwaita's unreleased creation reference.
    if let Some(buffer) = buffer.filter(|buffer| buffer.ref_count() == 3) {
        // SAFETY: release only the extra reference; GtkText and this handle keep
        // theirs. A library that releases its creation reference has a count of 2.
        unsafe { gtk::glib::gobject_ffi::g_object_unref(buffer.as_ptr().cast()) };
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    #[ignore = "requires an isolated GTK session"]
    fn password_buffers_finalize_with_their_rows() {
        gtk::init().unwrap();
        adw::init().unwrap();
        struct Finalized(Rc<Cell<usize>>);
        impl Drop for Finalized {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let finalized = Rc::new(Cell::new(0));
        for size in [0, 5, 25] {
            for _ in 0..25 {
                let row = password_row(adw::PasswordEntryRow::builder());
                row.set_text(&"x".repeat(size));
                let text = row.delegate().unwrap().downcast::<gtk::Text>().unwrap();
                let buffer = text.buffer();
                // SAFETY: the test owns this key and never reads its stored data.
                unsafe {
                    buffer.set_qdata(
                        gtk::glib::Quark::from_str("password-buffer-finalized"),
                        Finalized(finalized.clone()),
                    );
                }
                drop(buffer);
                drop(text);
                drop(row);
            }
        }
        assert_eq!(
            finalized.get(),
            75,
            "password buffers retained after row teardown"
        );
    }
}
