// SPDX-License-Identifier: GPL-3.0-or-later
//! Thin launcher for `big-video-converter`.

use std::ffi::OsString;
use std::process::{Command, ExitCode, ExitStatus};

const HOST_BINARY: &str = "big-host";
const HOST_BINARY_ENV: &str = "BIG_VIDEO_CONVERTER_HOST_BINARY";
const MODULE_ID: &str = "converter-video";

fn main() -> ExitCode {
    ExitCode::from(launch_host(std::env::args_os().skip(1)))
}

fn launch_host(args: impl IntoIterator<Item = OsString>) -> u8 {
    let host_binary = std::env::var_os(HOST_BINARY_ENV).unwrap_or_else(|| HOST_BINARY.into());
    launch_host_binary(host_binary, args)
}

fn launch_host_binary(host_binary: OsString, args: impl IntoIterator<Item = OsString>) -> u8 {
    let status = Command::new(host_binary)
        .arg("--separate")
        .arg(MODULE_ID)
        .args(args)
        .status();
    exit_status_code(status)
}

fn exit_status_code(status: std::io::Result<ExitStatus>) -> u8 {
    match status {
        Ok(status) => status.code().unwrap_or(1) as u8,
        Err(error) => {
            eprintln!("failed to launch {HOST_BINARY}: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    #[cfg(not(miri))]
    use std::sync::Mutex;

    #[cfg(not(miri))]
    static HOST_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(not(miri))]
    fn with_host_binary_override<T>(host_binary: &str, f: impl FnOnce() -> T) -> T {
        let _guard = HOST_ENV_LOCK.lock().expect("host env lock");
        let previous = std::env::var_os(HOST_BINARY_ENV);
        unsafe {
            std::env::set_var(HOST_BINARY_ENV, host_binary);
        }
        let result = f();
        unsafe {
            if let Some(previous) = previous {
                std::env::set_var(HOST_BINARY_ENV, previous);
            } else {
                std::env::remove_var(HOST_BINARY_ENV);
            }
        }
        result
    }

    #[test]
    fn exit_status_code_reports_spawn_error() {
        let code = exit_status_code(Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing host binary",
        )));

        assert_eq!(code, 1);
    }

    #[cfg(unix)]
    #[test]
    fn exit_status_code_maps_unix_status() {
        use std::os::unix::process::ExitStatusExt;

        assert_eq!(exit_status_code(Ok(ExitStatus::from_raw(0))), 0);
        assert_eq!(exit_status_code(Ok(ExitStatus::from_raw(7 << 8))), 7);
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
