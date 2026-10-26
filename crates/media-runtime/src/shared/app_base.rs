//! Bootstrap helpers shared by all binaries: env, gettext, locale,
//! GResource registration, decoration-icon strip, About dialog builder.

use gtk::gio;
use gtk::glib;

use crate::config;
use crate::settings::{self, keys};
use crate::shared::theming;

/// Set an environment variable only when it is currently unset.
pub fn set_default_env(name: &str, value: &str) {
    if std::env::var_os(name).is_none() {
        // SAFETY: callers run this from main() before any other thread starts.
        unsafe {
            std::env::set_var(name, value);
        }
    }
}

/// Set an environment variable during bootstrap regardless of previous value.
pub fn set_bootstrap_env(name: &str, value: &str) {
    // SAFETY: callers run this from main() before any other thread starts.
    unsafe {
        std::env::set_var(name, value);
    }
}

/// Render with GTK's GL renderer and keep GDK from opening Vulkan.
///
/// GDK creates a Vulkan instance to probe dmabuf formats even when GSK renders
/// with GL, and on NVIDIA that instance alone holds tens of MiB. GL imports
/// dmabufs through EGL instead, so nothing is lost. `force` overrides a `GSK_RENDERER`
/// the session exported; otherwise it is respected, and Vulkan stays available
/// if that is the renderer it asked for.
pub fn use_gl_renderer(force: bool) {
    if force {
        set_bootstrap_env("GSK_RENDERER", "opengl");
    } else {
        set_default_env("GSK_RENDERER", "opengl");
    }
    if std::env::var("GSK_RENDERER").is_ok_and(|renderer| renderer == "vulkan") {
        return;
    }
    let disabled = std::env::var("GDK_DISABLE").unwrap_or_default();
    if !disabled
        .split(',')
        .any(|feature| feature.trim() == "vulkan")
    {
        let value = if disabled.is_empty() {
            "vulkan".to_owned()
        } else {
            format!("{disabled},vulkan")
        };
        set_bootstrap_env("GDK_DISABLE", &value);
    }
}

/// Drop pointer motion that repeats the last position in `window`.
///
/// After every layout GTK re-delivers the pointer position as a motion event
/// so hover can follow moved widgets. A moving seek knob relayouts about once
/// a second, and each repeat reaches the list under a resting pointer, whose
/// overlay scrollbar then fades in and out: the window redraws continuously
/// while music plays. Hover itself is set by crossing events, which GTK computes before this
/// capture-phase controller sees the motion, so nothing stops following it.
pub fn ignore_repeated_pointer_motion(window: &impl glib::object::IsA<gtk::Window>) {
    use gtk::prelude::*;

    let last = std::cell::Cell::new(None::<(f64, f64)>);
    let filter = gtk::EventControllerLegacy::new();
    filter.set_propagation_phase(gtk::PropagationPhase::Capture);
    filter.connect_event(move |_, event| {
        match event.event_type() {
            gtk::gdk::EventType::MotionNotify => {
                let position = event.position();
                if position.is_some() && position == last.get() {
                    return glib::Propagation::Stop;
                }
                last.set(position);
            }
            gtk::gdk::EventType::EnterNotify | gtk::gdk::EventType::LeaveNotify => last.set(None),
            _ => {}
        }
        glib::Propagation::Proceed
    });
    window.as_ref().add_controller(filter);
}

/// Log which GSK renderer the display actually chose, once, when `window` maps.
///
/// The players pin `GSK_RENDERER` at boot, but GTK silently falls back when the
/// pinned renderer cannot be created, and the fallback decides whether frames
/// reach the compositor as a GPU dmabuf (`ngl`/`gl`) or as a per-frame software
/// upload (`cairo`), which is the dominant per-frame CPU cost on an old GPU.
/// This is the only place that says which one is really running.
pub fn log_active_renderer_once(window: &impl glib::object::IsA<gtk::Window>) {
    use gtk::prelude::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    static LOGGED: AtomicBool = AtomicBool::new(false);
    window.as_ref().connect_map(move |window| {
        if LOGGED.swap(true, Ordering::Relaxed) {
            return;
        }
        let renderer = window
            .renderer()
            .map_or_else(|| "unknown".to_owned(), |r| r.type_().name().to_string());
        let pinned =
            std::env::var("GSK_RENDERER").unwrap_or_else(|_| "unset (GTK default)".to_owned());
        log::info!("GSK renderer in use: {renderer} (GSK_RENDERER={pinned})");
    });
}

pub fn init_logging(default_rust_log: &str) {
    set_default_env("RUST_LOG", default_rust_log);
    env_logger::init();
}

/// Falls back to `<repo>/locale` so `cargo run` works without an install step.
pub fn init_gettext() {
    // SAFETY: called from main() before any other thread starts, as
    // init_numeric_locale is.
    unsafe {
        gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "");
    }
    let locale_dir = resolve_locale_dir();
    gettextrs::bindtextdomain(config::GETTEXT_PACKAGE, locale_dir)
        .expect("Failed to bind textdomain");
    gettextrs::textdomain(config::GETTEXT_PACKAGE).expect("Failed to set textdomain");
}

/// Probe pt_BR (maintainer locale): `/usr/share/locale` exists on Linux even
/// when our `.mo` files are not installed there.
fn resolve_locale_dir() -> String {
    // Each player packages its own copy of the shared catalog under a private
    // dir (`/usr/share/<exe>/locale`) so the two co-installable packages never
    // fight over one file in the system locale tree. Prefer it when present.
    if let Some(private) = private_locale_dir() {
        return private;
    }
    let installed = std::path::Path::new(config::LOCALEDIR)
        .join("pt_BR/LC_MESSAGES")
        .join(format!("{}.mo", config::GETTEXT_PACKAGE));
    if installed.exists() {
        return config::LOCALEDIR.to_string();
    }
    if let Some(dev) = dev_locale_dir() {
        return dev;
    }
    config::LOCALEDIR.to_string()
}

/// `/usr/share/<exe-stem>/locale`, if it holds our installed catalog.
fn private_locale_dir() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_str()?;
    let dir = format!("/usr/share/{stem}/locale");
    let probe = std::path::Path::new(&dir)
        .join("pt_BR/LC_MESSAGES")
        .join(format!("{}.mo", config::GETTEXT_PACKAGE));
    probe.exists().then_some(dir)
}

fn dev_locale_dir() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    for rel in ["../../locale", "../locale", "locale"] {
        let candidate = exe_dir.join(rel);
        if candidate
            .join("pt_BR/LC_MESSAGES")
            .join(format!("{}.mo", config::GETTEXT_PACKAGE))
            .exists()
        {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

/// libmpv parses numbers via libc and breaks under non-C decimal separators.
pub fn init_numeric_locale() {
    // GTK must preserve the per-category locale set by gettext + libmpv boot,
    // including when LC_ALL overrides LC_NUMERIC in the inherited environment.
    if !gtk::is_initialized() {
        gtk::disable_setlocale();
    }
    set_bootstrap_env("LC_NUMERIC", "C");
    let c_locale = std::ffi::CString::new("C").expect("static string has no null bytes");
    // SAFETY: called from main() before any other thread starts.
    unsafe {
        libc::setlocale(libc::LC_NUMERIC, c_locale.as_ptr());
    }
}

/// Boot steps every player entry point (standalone and embedded) shares: bind
/// the shared gettext domain, route the components' strings through it, pin
/// the numeric locale, and register the player's GResource bundles.
pub fn boot_player(resource_bundles: &[&'static [u8]]) {
    init_gettext();
    big_media_components::install_media_components_translator(crate::i18n::i18n);
    init_numeric_locale();
    for bundle in resource_bundles {
        register_resource_bytes(bundle);
    }
}

pub fn register_resource_bytes(resource_bytes: &'static [u8]) {
    let resource_blob = glib::Bytes::from_static(resource_bytes);
    let resource = gio::Resource::from_data(&resource_blob).expect("Failed to load resources");
    gio::resources_register(&resource);
}

pub fn apply_startup_color_scheme(
    settings: &settings::Settings,
    color_source_key: keys::StringKey,
) {
    theming::apply_color_scheme(&settings.get_string(color_source_key));
}

/// KDE Plasma's `icon:minimize,maximize,close` makes AdwHeaderBar render
/// the app icon, which libadwaita apps never show. Strip the token.
pub fn strip_decoration_icon() {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let Some(layout) = settings.gtk_decoration_layout() else {
        return;
    };
    if layout.contains("icon") {
        settings.set_gtk_decoration_layout(Some(&strip_icon_from_layout(&layout)));
    }
}

pub struct AboutDialogConfig {
    pub app_name: &'static str,
    pub app_icon: &'static str,
    /// Empty `""` to omit the comments line; mark it with `gettext_noop`, the
    /// dialog translates it.
    pub comments: &'static str,
}

/// Returns the dialog **before** presenting so callers can append credits.
pub fn build_about_dialog(about_dialog_config: &AboutDialogConfig) -> adw::AboutDialog {
    let mut builder = adw::AboutDialog::builder()
        .application_name(about_dialog_config.app_name)
        .application_icon(about_dialog_config.app_icon)
        .developer_name("BigLinux")
        .version(crate::config::APP_VERSION)
        .website("https://biglinux.com.br")
        .license_type(gtk::License::Gpl30);

    if !about_dialog_config.comments.is_empty() {
        builder = builder.comments(crate::i18n::i18n(about_dialog_config.comments));
    }

    builder.build()
}

fn strip_icon_from_layout(raw: &str) -> String {
    raw.split(':')
        .map(|side| {
            side.split(',')
                .filter(|tok| *tok != "icon")
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_icon_kde_layout() {
        assert_eq!(
            strip_icon_from_layout("icon:minimize,maximize,close"),
            ":minimize,maximize,close"
        );
    }

    #[test]
    fn strip_icon_no_icon_present() {
        let layout = "minimize,maximize:close";
        assert_eq!(strip_icon_from_layout(layout), layout);
    }

    #[test]
    fn strip_icon_icon_only_left() {
        assert_eq!(strip_icon_from_layout("icon:close"), ":close");
    }

    #[test]
    fn strip_icon_icon_in_middle() {
        assert_eq!(
            strip_icon_from_layout("icon,minimize:maximize,close"),
            "minimize:maximize,close"
        );
    }

    #[test]
    fn strip_icon_empty_layout() {
        assert_eq!(strip_icon_from_layout(""), "");
    }

    #[test]
    fn strip_icon_no_colon() {
        assert_eq!(
            strip_icon_from_layout("icon,minimize,close"),
            "minimize,close"
        );
    }
}
