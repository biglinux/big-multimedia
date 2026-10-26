/// Repository that owns `APP_VERSION`, for gitrepo.
pub const APP_VERSION_OWNER: &str = "big-multimedia";
/// Version of both players, shown in their About dialogs. gitrepo bumps it
/// from each commit it sends (`feat` raises the minor, `fix` and friends the
/// patch, a breaking change the major); set it by hand only for a planned
/// release.
pub const APP_VERSION: &str = "2.0.0";

pub const GETTEXT_PACKAGE: &str = runtime::GETTEXT_PACKAGE;
pub const LOCALEDIR: &str = "/usr/share/locale";

pub const SHADERS_DIR: &str = runtime::SHADERS_DIR;

pub mod runtime {
    pub const CONFIG_DIR: &str = "big-media-runtime";
    pub const GETTEXT_PACKAGE: &str = "big-media-runtime";
    pub const SHADERS_DIR: &str = "/usr/share/big-media-runtime/shaders";
}

pub mod video {
    pub const APP_ID: &str = "br.com.biglinux.VideoPlayer";
    pub const APP_NAME: &str = "Big Video Player";
    /// Themed icon installed by the package and named by the desktop file.
    pub const ICON_NAME: &str = "big-video-player";
    pub const CONFIG_DIR: &str = "big-video-player";
    pub const RESOURCE_PREFIX: &str = "/br/com/biglinux/VideoPlayer";
    pub const WATCH_LATER_RELATIVE: &str = "big-video-player/watch_later";
    pub const USER_SHADERS_RELATIVE: &str = "big-video-player/shaders";
}

pub mod audio {
    pub const APP_ID: &str = "br.com.biglinux.AudioPlayer";
    pub const APP_NAME: &str = "Big Audio Player";
    /// Themed icon installed by the package and named by the desktop file.
    pub const ICON_NAME: &str = "big-audio-player";
    /// One-color icon of the system tray, recolored by the panel's theme.
    pub const TRAY_ICON_NAME: &str = "big-audio-player-symbolic";
    pub const CONFIG_DIR: &str = "big-audio-player";
    pub const RESOURCE_PREFIX: &str = "/br/com/biglinux/AudioPlayer";
    pub const WATCH_LATER_RELATIVE: &str = "big-audio-player/watch_later";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_version_is_plain_semver_for_gitrepo() {
        let parts: Vec<&str> = APP_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "{APP_VERSION} is not MAJOR.MINOR.PATCH");
        assert!(parts.iter().all(|part| part.parse::<u32>().is_ok()));
    }

    #[test]
    fn resource_prefixes_start_with_slash() {
        assert!(video::RESOURCE_PREFIX.starts_with('/'));
        assert!(audio::RESOURCE_PREFIX.starts_with('/'));
    }

    #[test]
    fn gettext_package_matches_crate() {
        assert_eq!(GETTEXT_PACKAGE, "big-media-runtime");
    }
}
