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
    pub const CONFIG_DIR: &str = "big-video-player";
    pub const RESOURCE_PREFIX: &str = "/br/com/biglinux/VideoPlayer";
    pub const WATCH_LATER_RELATIVE: &str = "big-video-player/watch_later";
    pub const USER_SHADERS_RELATIVE: &str = "big-video-player/shaders";
}

pub mod audio {
    pub const APP_ID: &str = "br.com.biglinux.AudioPlayer";
    pub const APP_NAME: &str = "Big Audio Player";
    pub const CONFIG_DIR: &str = "big-audio-player";
    pub const RESOURCE_PREFIX: &str = "/br/com/biglinux/AudioPlayer";
    pub const WATCH_LATER_RELATIVE: &str = "big-audio-player/watch_later";
}

#[cfg(test)]
mod tests {
    use super::*;

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
