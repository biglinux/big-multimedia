//! Persist `gtk::ColumnView` column widths to settings as a
//! `title=width,…` string keyed by a settings `StringKey`.
//! bigagents: app-local-adapter; storage is runtime settings, encoding lives
//! in `big_relm4_components::layout::column_persist`.

pub fn save_column_widths(
    column_view: &gtk::ColumnView,
    settings: &crate::settings::Settings,
    key: crate::settings::keys::StringKey,
) {
    if let Some(widths) =
        big_relm4_components::layout::column_persist::column_view_widths_string(column_view)
    {
        settings.put_string(key, &widths);
    }
}
