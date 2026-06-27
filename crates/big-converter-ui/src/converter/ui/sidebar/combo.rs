//! Sidebar ComboRow factory + parallel-ID-table accessors.

use adw::prelude::*;
use big_relm4_components::input::combo_row::{BigComboRow, BigComboRowSpec};

/// Build an `adw::ComboRow` with `title` and an initial
/// [`gtk::StringList`] model populated from `labels`.
pub fn make_combo(title: &str, labels: &[&str]) -> adw::ComboRow {
    BigComboRow::new(BigComboRowSpec::new(title, labels.iter().copied())).into_root()
}

/// Build an `adw::ComboRow` without an initial model.
pub fn make_empty_combo(title: &str) -> adw::ComboRow {
    BigComboRow::new(BigComboRowSpec::empty(title)).into_root()
}

/// Select the row in `combo` whose id matches `target`, looked up in
/// the parallel `ids` table. No-op when `target` is not present.
pub fn set_combo_by_id(combo: &adw::ComboRow, ids: &[&str], target: &str) {
    if let Some(pos) = ids.iter().position(|&id| id == target) {
        combo.set_selected(pos as u32);
    }
}

/// Read the id of the row currently selected in `combo` using the
/// parallel `ids` table. Falls back to `ids[0]` if the selected index
/// is out of range.
pub fn combo_id(combo: &adw::ComboRow, ids: &[&str]) -> String {
    let idx = combo.selected() as usize;
    ids.get(idx).unwrap_or(&ids[0]).to_string()
}
