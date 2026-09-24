//! Launch the external converter GUI (big-audio-converter-gui, fallback to
//! big-video-converter-gui with an audio-mode env hint) for a list of media
//! paths. The plain `big-video-converter` binary is a headless CLI, not a GUI.

use std::path::Path;

use big_app_kit::subprocess::BigSubprocessSpec;

const AUDIO_CONVERTER_PROGRAM: &str = "big-audio-converter-gui";
const VIDEO_CONVERTER_PROGRAM: &str = "big-video-converter-gui";
const AUDIO_MODE_ENV_KEY: &str = "BIG_MEDIA_AUDIO_CONVERTER";
const AUDIO_MODE_ENV_VALUE: &str = "1";
const END_OF_OPTIONS_MARKER: &str = "--";
const CONVERTER_BINARY_CANDIDATES: &[&str] = &[
    "/usr/bin/big-audio-converter-gui",
    "/usr/sbin/big-audio-converter-gui",
    "/usr/bin/big-video-converter-gui",
    "/usr/sbin/big-video-converter-gui",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConverterLaunchCommandSpec {
    program: &'static str,
    audio_mode_env: Option<(&'static str, &'static str)>,
}

fn converter_binary_candidates() -> &'static [&'static str] {
    CONVERTER_BINARY_CANDIDATES
}

fn is_converter_binary_available(binary_exists: impl Fn(&Path) -> bool) -> bool {
    converter_binary_candidates()
        .iter()
        .map(Path::new)
        .any(binary_exists)
}

fn converter_launch_command_specs() -> [ConverterLaunchCommandSpec; 2] {
    [
        ConverterLaunchCommandSpec {
            program: AUDIO_CONVERTER_PROGRAM,
            audio_mode_env: None,
        },
        ConverterLaunchCommandSpec {
            program: VIDEO_CONVERTER_PROGRAM,
            audio_mode_env: Some((AUDIO_MODE_ENV_KEY, AUDIO_MODE_ENV_VALUE)),
        },
    ]
}

fn build_converter_subprocess(
    command_spec: ConverterLaunchCommandSpec,
    paths: &[String],
) -> BigSubprocessSpec {
    let mut builder = BigSubprocessSpec::builder()
        .program(command_spec.program)
        .arg(END_OF_OPTIONS_MARKER)
        .args(paths);
    if let Some((key, value)) = command_spec.audio_mode_env {
        builder = builder.env(key, value);
    }
    builder.build()
}

fn converter_launch_subprocesses(paths: &[String]) -> [BigSubprocessSpec; 2] {
    let [primary_spec, fallback_spec] = converter_launch_command_specs();
    [
        build_converter_subprocess(primary_spec, paths),
        build_converter_subprocess(fallback_spec, paths),
    ]
}

pub(super) fn is_available() -> bool {
    is_converter_binary_available(Path::exists)
}

/// Falls back to `big-video-converter-gui` with `BIG_MEDIA_AUDIO_CONVERTER=1`.
///
/// SECURITY: every user-supplied path is placed after a `--` end-of-options
/// marker so file names beginning with `-` cannot be reinterpreted as
/// command-line flags by the converter binary.
pub(super) fn launch(paths: &[String]) {
    let [primary_subprocess, fallback_subprocess] = converter_launch_subprocesses(paths);
    if let Err(primary_err) = primary_subprocess.spawn_detached()
        && let Err(fallback_err) = fallback_subprocess.spawn_detached()
    {
        log::warn!(
            "failed to launch audio converter (primary: {primary_err}; fallback: {fallback_err})"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_subprocess(
        subprocess: &BigSubprocessSpec,
    ) -> big_app_kit::subprocess::BigSubprocessResolved {
        subprocess.resolved()
    }

    #[test]
    fn converter_binary_candidates_cover_audio_then_video_paths() {
        assert_eq!(
            converter_binary_candidates(),
            &[
                "/usr/bin/big-audio-converter-gui",
                "/usr/sbin/big-audio-converter-gui",
                "/usr/bin/big-video-converter-gui",
                "/usr/sbin/big-video-converter-gui",
            ]
        );
    }

    #[test]
    fn converter_availability_uses_any_known_binary() {
        for available_binary in converter_binary_candidates() {
            assert!(is_converter_binary_available(
                |candidate| candidate == Path::new(available_binary)
            ));
        }

        assert!(!is_converter_binary_available(|_| false));
    }

    #[test]
    fn converter_launch_specs_try_audio_before_video_audio_mode() {
        assert_eq!(
            converter_launch_command_specs(),
            [
                ConverterLaunchCommandSpec {
                    program: AUDIO_CONVERTER_PROGRAM,
                    audio_mode_env: None,
                },
                ConverterLaunchCommandSpec {
                    program: VIDEO_CONVERTER_PROGRAM,
                    audio_mode_env: Some((AUDIO_MODE_ENV_KEY, AUDIO_MODE_ENV_VALUE)),
                },
            ]
        );
    }

    #[test]
    fn converter_subprocess_specs_preserve_paths_after_option_marker() {
        let paths = vec![
            "/music/track.flac".to_string(),
            "-leading-dash.wav".to_string(),
        ];
        let [primary_subprocess, fallback_subprocess] = converter_launch_subprocesses(&paths);
        let primary = resolve_subprocess(&primary_subprocess);
        let fallback = resolve_subprocess(&fallback_subprocess);

        assert_eq!(primary.program, AUDIO_CONVERTER_PROGRAM);
        assert_eq!(
            primary.argv,
            vec![
                END_OF_OPTIONS_MARKER.to_string(),
                "/music/track.flac".to_string(),
                "-leading-dash.wav".to_string(),
            ]
        );
        assert!(primary.env_overrides.is_empty());

        assert_eq!(fallback.program, VIDEO_CONVERTER_PROGRAM);
        assert_eq!(fallback.argv, primary.argv);
        assert_eq!(
            fallback.env_overrides,
            vec![(
                AUDIO_MODE_ENV_KEY.to_string(),
                AUDIO_MODE_ENV_VALUE.to_string()
            )]
        );
    }
}
