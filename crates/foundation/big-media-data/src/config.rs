pub const GETTEXT_PACKAGE: &str = runtime::GETTEXT_PACKAGE;
pub const LOCALEDIR: &str = "/usr/share/locale";

pub const SHADERS_DIR: &str = runtime::SHADERS_DIR;

pub mod runtime {
    pub const CONFIG_DIR: &str = "big-media-runtime";
    pub const GETTEXT_PACKAGE: &str = "big-media-runtime";
    pub const SYSTEM_DATA_DIR: &str = "/usr/share/big-media-runtime";
    pub const SHADERS_DIR: &str = "/usr/share/big-media-runtime/shaders";
    pub const CONVERTER_CONFIG_PATH: &str = "/usr/share/big-media-runtime/converter-runtime.toml";
    pub const CONVERTER_CONFIG_RELATIVE: &str = "big-media-runtime/converter-runtime.toml";
    pub const PROFILES_DIR: &str = "/usr/share/big-media-runtime/profiles";
    pub const USER_PROFILES_RELATIVE: &str = "big-media-runtime/profiles";
}

pub mod video {
    pub const APP_ID: &str = "br.com.biglinux.VideoPlayer";
    pub const APP_NAME: &str = "Big Video Player";
    pub const CONFIG_DIR: &str = "big-video-player";
    pub const SETTINGS_EXPORT_NAME: &str = "big-video-player-settings.conf";
    pub const RESOURCE_PREFIX: &str = "/br/com/biglinux/VideoPlayer";
    pub const WATCH_LATER_RELATIVE: &str = "big-video-player/watch_later";
    pub const MPV_IPC_SOCKET: &str = "big-video-player.sock";
    pub const USER_SHADERS_RELATIVE: &str = "big-video-player/shaders";
}

pub mod audio {
    pub const APP_ID: &str = "br.com.biglinux.AudioPlayer";
    pub const APP_NAME: &str = "Big Audio Player";
    pub const CONFIG_DIR: &str = "big-audio-player";
    pub const RESOURCE_PREFIX: &str = "/br/com/biglinux/AudioPlayer";
    pub const WATCH_LATER_RELATIVE: &str = "big-audio-player/watch_later";
}

pub mod video_converter {
    pub const APP_ID: &str = "br.com.biglinux.VideoConverter";
    pub const APP_NAME: &str = "Big Video Converter";
    pub const CONFIG_DIR: &str = "big-video-converter";
}

pub mod audio_converter {
    pub const APP_ID: &str = "br.com.biglinux.AudioConverter";
    pub const APP_NAME: &str = "Big Audio Converter";
    pub const CONFIG_DIR: &str = "big-audio-converter";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_are_valid_dbus_names() {
        // D-Bus well-known names: dot-separated segments, no segment may start with a digit.
        for id in [
            video::APP_ID,
            audio::APP_ID,
            video_converter::APP_ID,
            audio_converter::APP_ID,
        ] {
            let parts: Vec<&str> = id.split('.').collect();
            assert!(parts.len() >= 3, "APP_ID needs ≥3 segments: {id}");
            for part in &parts {
                assert!(!part.is_empty(), "empty segment in APP_ID: {id}");
                assert!(
                    !part.starts_with(char::is_numeric),
                    "segment starts with digit in APP_ID: {id}"
                );
            }
        }
    }

    #[test]
    fn resource_prefixes_start_with_slash() {
        assert!(video::RESOURCE_PREFIX.starts_with('/'));
        assert!(audio::RESOURCE_PREFIX.starts_with('/'));
    }

    #[test]
    fn app_names_not_empty() {
        assert!(!video::APP_NAME.is_empty());
        assert!(!audio::APP_NAME.is_empty());
        assert!(!video_converter::APP_NAME.is_empty());
        assert!(!audio_converter::APP_NAME.is_empty());
    }

    #[test]
    fn gettext_package_matches_crate() {
        assert_eq!(GETTEXT_PACKAGE, "big-media-runtime");
    }
}
