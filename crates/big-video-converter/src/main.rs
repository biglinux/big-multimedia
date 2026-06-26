// SPDX-License-Identifier: GPL-3.0-or-later
//! Thin launcher for `big-video-converter`.

use std::ffi::OsString;
use std::process::ExitCode;

const HOST_BINARY: &str = "big-host";
const HOST_BINARY_ENV: &str = "BIG_VIDEO_CONVERTER_HOST_BINARY";
const MODULE_ID: &str = "converter-video";

fn main() -> ExitCode {
    ExitCode::from(launch_host(std::env::args_os().skip(1)))
}

fn launch_host(args: impl IntoIterator<Item = OsString>) -> u8 {
    launch_host_binary(host_binary_from_env(), args)
}

fn host_binary_from_env() -> OsString {
    std::env::var_os(HOST_BINARY_ENV).unwrap_or_else(|| HOST_BINARY.into())
}

fn launch_host_binary(host_binary: OsString, args: impl IntoIterator<Item = OsString>) -> u8 {
    big_converter_ui::launch_host_module_code(host_binary, MODULE_ID, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(miri))]
    use std::sync::Mutex;

    #[cfg(not(miri))]
    static HOST_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(not(miri))]
    fn with_host_binary_override<T>(host_binary: &str, f: impl FnOnce() -> T) -> T {
        let _guard = HOST_ENV_LOCK.lock().expect("host env lock");
        let previous = std::env::var_os(HOST_BINARY_ENV);
        // SAFETY: HOST_ENV_LOCK serializes every env mutation in this test
        // helper, so no other thread reads/writes HOST_BINARY_ENV concurrently
        // (the soundness condition for the Rust 2024 `set_var`).
        unsafe {
            std::env::set_var(HOST_BINARY_ENV, host_binary);
        }
        let result = f();
        // SAFETY: still holding HOST_ENV_LOCK — same serialization as above.
        unsafe {
            if let Some(previous) = previous {
                std::env::set_var(HOST_BINARY_ENV, previous);
            } else {
                std::env::remove_var(HOST_BINARY_ENV);
            }
        }
        result
    }

    #[cfg(not(miri))]
    #[test]
    fn launch_host_binary_reports_spawn_error() {
        let code = launch_host_binary(
            OsString::from("/definitely/missing/big-video-converter-host"),
            std::iter::empty::<OsString>(),
        );

        assert_eq!(code, 1);
    }

    #[cfg(not(miri))]
    #[test]
    fn launch_host_binary_rejects_control_byte_arg() {
        let code = launch_host_binary("true".into(), [OsString::from("bad\narg")]);

        assert_eq!(code, 1);
    }

    #[cfg(not(miri))]
    #[test]
    fn launch_host_reports_success() {
        let code = with_host_binary_override("true", || launch_host(std::iter::empty()));

        assert_eq!(code, 0);
    }

    #[cfg(not(miri))]
    #[test]
    fn launch_host_reports_failure() {
        let code = with_host_binary_override("false", || launch_host(std::iter::empty()));

        assert_ne!(code, 0);
    }

    #[cfg(not(miri))]
    #[test]
    fn launch_host_binary_forwards_separate_module_args() {
        let code = launch_host_binary("true".into(), [OsString::from("/tmp/input.mkv")]);

        assert_eq!(code, 0);
    }
}
