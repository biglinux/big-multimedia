// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Display-free dialog helpers.

/// Persist a positive dialog size through a caller-owned storage closure.
///
/// Returns `false` when GTK has not reported a usable size yet. Callers own
/// settings keys, debounce policy, and error handling.
pub fn persist_dialog_size(width: i32, height: i32, persist: impl FnOnce(i32, i32)) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    persist(width, height);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persist_dialog_size_requires_positive_dimensions() {
        let mut persisted_size = None;

        assert!(!persist_dialog_size(0, 480, |width, height| {
            persisted_size = Some((width, height));
        }));
        assert_eq!(persisted_size, None);

        assert!(!persist_dialog_size(640, -1, |width, height| {
            persisted_size = Some((width, height));
        }));
        assert_eq!(persisted_size, None);

        assert!(persist_dialog_size(640, 480, |width, height| {
            persisted_size = Some((width, height));
        }));
        assert_eq!(persisted_size, Some((640, 480)));
    }
}
