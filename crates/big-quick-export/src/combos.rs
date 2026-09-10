//! ComboRow factory + parallel-ID-table accessors used by [`crate::encode_options`].
//!
//! IDs are machine-readable (never translated); user-visible LABELS come from
//! [`big_media_runtime::ui_labels`] so every string flows through `i18n()`.

use adw::prelude::*;
use big_relm4_components::input::combo_row::{BigComboRow, BigComboRowSpec};

/// Video-codec id table; index matches the codec combo row order.
pub const CODEC_IDS: &[&str] = &["copy", "h264", "h265", "av1", "vp9", "prores"];
/// Video quality preset id table.
pub const QUALITY_IDS: &[&str] = &[
    "veryhigh", "high", "good", "medium", "low", "verylow", "superlow",
];
/// Container/extension id table for video mode.
pub const CONTAINER_IDS: &[&str] = &["mp4", "mkv", "mov", "webm"];

/// Build an `adw::ComboRow` with `title` and an initial [`gtk::StringList`]
/// model populated from `labels`.
pub fn make_combo(title: &str, labels: &[&str]) -> adw::ComboRow {
    BigComboRow::new(BigComboRowSpec::new(title, labels.iter().copied())).into_root()
}

/// Select the row in `combo` whose id matches `target`, looked up in the
/// parallel `ids` table. No-op when `target` is not present.
pub fn set_combo_by_id(combo: &adw::ComboRow, ids: &[&str], target: &str) {
    if let Some(pos) = ids.iter().position(|&id| id == target) {
        combo.set_selected(pos as u32);
    }
}

/// Read the id of the row currently selected in `combo` using the parallel
/// `ids` table. Falls back to `ids[0]` if the selected index is out of range.
pub fn combo_id(combo: &adw::ComboRow, ids: &[&str]) -> String {
    let idx = combo.selected() as usize;
    ids.get(idx).unwrap_or(&ids[0]).to_string()
}

/// Turn an owned label `Vec<String>` into a `Vec<&str>` for APIs that take
/// `&[&str]`. Caller must keep the owned vec alive for the slice's life.
#[must_use]
pub fn as_str_refs(labels: &[String]) -> Vec<&str> {
    labels.iter().map(String::as_str).collect()
}
