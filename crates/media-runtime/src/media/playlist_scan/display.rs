use std::path::Path;

use crate::i18n::i18n;

pub fn folder_display_name(path: &str) -> String {
    big_app_kit::files::folder_display_name(path)
}

/// Returns `(display_name, full_path)` per ancestor up to `~` or `/`; up to
/// `/` when `$HOME` is unset or relative.
pub fn breadcrumb_segments(folder: &str) -> Vec<(String, String)> {
    let path = Path::new(folder);
    let home = big_os_kit::xdg_dirs::home_dir();
    let mut segments = Vec::new();
    let mut current = path.to_path_buf();

    loop {
        let at_home = home.as_ref() == Some(&current);
        let name = if at_home {
            i18n("Home")
        } else {
            current.file_name().map_or_else(
                || "/".to_string(),
                |name| name.to_string_lossy().to_string(),
            )
        };
        segments.push((name, current.to_string_lossy().to_string()));

        if at_home || current.parent().is_none() || current == Path::new("/") {
            break;
        }
        current = current
            .parent()
            .expect("guarded by is_none check above")
            .to_path_buf();
    }

    segments.reverse();
    segments
}
