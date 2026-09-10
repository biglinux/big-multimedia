# BigLinux media daemon contract

`big-media-runtime` owns durable media engines and services. Apps present UI,
submit commands, and observe snapshots; the runtime owns long work, subprocess
control, progress parsing, cancellation, retry, and recovery.

## Daemons

| Daemon | Owner | Responsibility | Reattach |
|---|---|---|---|
| `big-convertd` | `big-media-runtime` | conversion, transcode, extract, queue, progress, output collision, cleanup | required |
| `bigcam-recordd` | `bigcam` or runtime adapter | camera/screen/microphone recording state and file finalization | required |
| `big-terminal-ptyd` | `big-terminal` | shell and SSH PTY sessions | required |
| `big-fileopsd` | `big-filemanager` | copy, move, delete, trash, remote file operations | required |
| `big-camerad` | future runtime/bigcam | camera leases and capabilities | conditional |
| `big-indexd` | shell/search owner | optional file/media indexing | conditional |

## Common Requirements

- Use `big-os-kit::microdaemon` framing or a documented versioned adapter.
- Store compact job/session state in an atomic journal.
- Support submit, list, attach, cancel, restart, recover, and cleanup.
- UI close/reopen must show the same in-progress or completed durable jobs.
- Cancellation must signal the correct child process or process group and clean
  temporary files safely.
- Logs are short, redacted, and tied to a job id.

## Media Runtime Responsibilities

- `big-convertd` is the single owner of converter queue state.
- FFmpeg progress parsing has one canonical implementation.
- Output path collision handling has one canonical implementation.
- mpv/GStreamer/FFmpeg commands that can block run outside the GTK main loop.
- Camera source descriptors shared with `bigcam` use one canonical data model.

## Prohibited

- UI-owned FFmpeg/mpv/GStreamer subprocess lifetimes for durable work.
- Local progress parsers in converters or players when the canonical parser is
  available.
- Blocking UI handlers with media probing, transcoding, metadata extraction, or
  device enumeration.
- Persisting GTK widgets, paintables, GPU handles, D-Bus proxies, or secrets in
  daemon journals.

## Acceptance

- Converter UI submits to `big-convertd` and renders snapshots/events.
- Reopen after UI close lists previous in-progress and completed jobs.
- Crash/restart tests cover daemon journal recovery.
- Deny-path tests cover invalid frames, unsafe output paths, cancellation races,
  missing tools, and hostile media filenames.
- `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --all-targets --locked`, and
  `cargo doc --workspace --no-deps --locked` pass before handoff.
