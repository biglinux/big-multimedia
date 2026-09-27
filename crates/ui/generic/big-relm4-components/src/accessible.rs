//! Accessibility relations that name other widgets.

use gtk::glib;
use gtk::glib::translate::{IntoGlib, ToGlibContainerFromSlice, ToGlibPtr, ToGlibPtrMut};
use gtk::prelude::*;

/// Say that `accessible` is `relation` (labelled by, described by, …) `targets`.
///
/// gtk-rs 0.11's `AccessibleExt::update_relation` hands GTK a `GList` of the
/// targets inside a `GValue` and never frees it, while GTK takes a copy of the
/// list (`gtkaccessiblevalue.c`): every call left one list behind, and the
/// rows of every window's menu made thousands. This frees it once GTK has
/// copied it.
pub fn relate(
    accessible: &impl IsA<gtk::Accessible>,
    relation: gtk::AccessibleRelation,
    targets: &[&gtk::Accessible],
) {
    let (list, _targets) =
        ToGlibContainerFromSlice::<*mut glib::ffi::GList>::to_glib_container_from_slice(targets);
    let mut value = glib::Value::from_type(glib::Type::POINTER);
    let mut relation = relation.into_glib();
    // SAFETY: `value` is a POINTER value; GTK reads the list during the call and
    // keeps a copy of it, so the list is ours to free afterwards. The targets
    // stay alive through `_targets` for the duration of the call.
    unsafe {
        glib::gobject_ffi::g_value_set_pointer(value.to_glib_none_mut().0, list.cast());
        gtk::ffi::gtk_accessible_update_relation_value(
            accessible.as_ref().to_glib_none().0,
            1,
            &raw mut relation,
            value.to_glib_none().0,
        );
        glib::ffi::g_list_free(list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs a display: run under scripts/headless-gate.sh"]
    fn the_relation_is_set() {
        gtk::init().unwrap();
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::new(Some("Name"));
        relate(
            &row,
            gtk::AccessibleRelation::LabelledBy,
            &[label.upcast_ref()],
        );
        assert!(gtk::test_accessible_has_relation(
            &row,
            gtk::AccessibleRelation::LabelledBy
        ));
    }
}
