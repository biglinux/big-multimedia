// SPDX-License-Identifier: GPL-3.0-or-later

//! Standalone converter launcher helpers.
//!
//! These functions are intentionally display-free. They are used by thin
//! standalone bindings that need to delegate to `big-host --separate <module>`
//! while keeping subprocess policy in the shared BigLinux component layer.

use std::ffi::OsString;
use std::process::ExitCode;

/// Launch a converter module through `big-host` and map the child status to
/// a process [`ExitCode`].
///
/// This keeps standalone bindings thin while still using the shared
/// BigLinux subprocess contract instead of app-local `std::process::Command`.
#[must_use]
pub fn launch_host_module<I, T>(
    host_binary: impl Into<OsString>,
    module_id: &str,
    args: I,
) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    ExitCode::from(launch_host_module_code(host_binary, module_id, args))
}

/// Launch a converter module through `big-host` and return a raw exit code.
///
/// Tests use this form because [`ExitCode`] intentionally does not expose the
/// platform process status back to ordinary Rust code.
#[must_use]
pub fn launch_host_module_code<I, T>(
    host_binary: impl Into<OsString>,
    module_id: &str,
    args: I,
) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let host_binary = host_binary.into();
    let spec = build_host_module_spec(host_binary.clone(), module_id, args);
    let host_label = host_binary.to_string_lossy();
    match spec.spawn().and_then(|mut child| child.wait()) {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1),
        Err(error) => {
            eprintln!("failed to launch {host_label}: {error}");
            1
        }
    }
}

fn build_host_module_spec<I, T>(
    host_binary: OsString,
    module_id: &str,
    args: I,
) -> big_app_kit::subprocess::BigSubprocessSpec
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    use big_app_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

    BigSubprocessSpec::builder()
        .program(host_binary.to_string_lossy().into_owned())
        .arg("--separate")
        .arg(module_id)
        .args(args)
        .stdout(BigSubprocessOutputMode::Inherit)
        .stderr(BigSubprocessOutputMode::Inherit)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_app_kit::subprocess::BigSubprocessOutputMode;

    #[test]
    fn host_module_spec_forwards_separate_module_args() {
        let spec = build_host_module_spec(
            OsString::from("big-host"),
            "converter-video",
            [OsString::from("/tmp/input.mkv")],
        );
        let resolved = spec.resolved();

        assert_eq!(resolved.program, "big-host");
        assert_eq!(
            resolved.argv,
            ["--separate", "converter-video", "/tmp/input.mkv"]
        );
        assert_eq!(resolved.stdout, BigSubprocessOutputMode::Inherit);
        assert_eq!(resolved.stderr, BigSubprocessOutputMode::Inherit);
    }

    #[test]
    #[cfg(not(miri))]
    fn launch_host_module_reports_success() {
        assert_eq!(
            launch_host_module_code("true", "converter-video", std::iter::empty::<OsString>()),
            0
        );
    }

    #[test]
    #[cfg(not(miri))]
    fn launch_host_module_reports_failure() {
        assert_ne!(
            launch_host_module_code("false", "converter-video", std::iter::empty::<OsString>()),
            0
        );
    }

    #[test]
    fn launch_host_module_rejects_control_byte_arg() {
        assert_eq!(
            launch_host_module_code("true", "converter-video", [OsString::from("bad\narg")]),
            1
        );
    }
}
