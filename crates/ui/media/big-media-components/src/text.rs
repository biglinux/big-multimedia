//! Process-local text hook for reusable GTK components.
//!
//! Components inside this crate never bind a gettext domain directly so
//! they can ship in a shared crate. Each app installs a translator
//! function via [`install_media_components_translator`] at startup, and
//! [`text`] funnels every UI string through it.

use std::cell::RefCell;
use std::rc::Rc;

type Translator = Rc<dyn Fn(&str) -> String>;

thread_local! {
    static TRANSLATOR: RefCell<Option<Translator>> = RefCell::new(None);
}

/// Install a translator callback for the current thread.
///
/// Subsequent calls to [`text`] route through `translator`. The closure
/// is stored as an `Rc<dyn Fn>` so it must be cheap to share. Repeated
/// calls replace the previously installed translator.
///
/// # Examples
///
/// ```
/// use big_media_components::install_media_components_translator;
///
/// install_media_components_translator(|message| {
///     // Hand off to gettext, fluent, or any other backend.
///     message.to_string()
/// });
/// ```
pub fn install_media_components_translator<F>(translator: F)
where
    F: Fn(&str) -> String + 'static,
{
    replace_media_components_translator(Some(Rc::new(translator)));
}

fn replace_media_components_translator(translator: Option<Translator>) {
    TRANSLATOR.with(|slot| {
        *slot.borrow_mut() = translator;
    });
}

#[cfg(test)]
fn clear_media_components_translator_for_tests() {
    replace_media_components_translator(None);
}

/// Translate `message` through the currently installed translator.
///
/// Returns `message.to_string()` when no translator has been installed,
/// keeping widgets usable in plain English during early startup and in
/// crate-local unit tests.
#[must_use]
pub fn text(message: &str) -> String {
    TRANSLATOR.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or_else(|| message.to_string(), |translator| translator(message))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        clear_media_components_translator_for_tests, install_media_components_translator, text,
    };

    #[test]
    fn text_returns_original_message_without_installed_translator() {
        clear_media_components_translator_for_tests();

        assert_eq!(text("Volume"), "Volume");
    }

    #[test]
    fn installed_translator_receives_message_text() {
        clear_media_components_translator_for_tests();
        install_media_components_translator(|message| format!("translated:{message}"));

        assert_eq!(text("Volume"), "translated:Volume");
    }

    #[test]
    fn repeated_install_replaces_previous_translator() {
        clear_media_components_translator_for_tests();
        install_media_components_translator(|message| format!("first:{message}"));
        install_media_components_translator(|message| format!("second:{message}"));

        assert_eq!(text("Volume"), "second:Volume");
    }
}
