# ARCHITECTURE — big-audio-converter

## 1. Purpose

`big-audio-converter` is a GTK4/libadwaita desktop application that lets a
user pick audio files, edit per-track parameters (trim, normalize, format),
queue them, and transcode via `ffmpeg`. The binary itself is a ~25-line
thin wrapper: nearly all behaviour lives in the shared
`big-converter-ui` crate (UI + state) and `big-media-runtime` crate
(ffmpeg orchestration, scratch dirs, probing). The audio binary boots the
UI in `ConverterMode::Audio` and flips the `BIG_MEDIA_AUDIO_CONVERTER=1`
bootstrap env so the shared crate gates audio-only code paths.

## 2. Crate layout

| Crate | Path | Role |
|---|---|---|
| `big-audio-converter` (bin+lib) | `src/main.rs`, `src/lib.rs` | Entry point. Calls `big_converter_ui::run(RunOptions { mode: Audio, … })`. |
| `big-converter-ui` (dep, path) | `../big-converter-ui` | Shared GTK UI: window, file list, edit page, queue, audio playback. |
| `big-media-runtime` (dev-dep) | `../big-media-runtime` | ffmpeg/ffprobe wrappers, cast/transcode session, scratch-dir lifecycle. Reused by tests. |
| `gtk4` 0.11 (v4_22) | crates.io | GTK bindings. |

The dep graph is intentionally minimal at the binary level: linking is
deferred to the shared component crate so the audio and video binaries
stay in lockstep.

## 3. State machine and message flow

Owned by `big-converter-ui::ConverterModel` (audio variant). High-level
states:

1. `Idle` — no files queued.
2. `Editing` — at least one file present; user adjusts per-track params
   on the edit page. Playback pump runs at 50 ms cadence and emits
   `Tick` while the `async_channel::Sender` is alive.
3. `Queued` — user presses Start. Items move into the runtime queue.
4. `Converting` — `big-media-runtime::converter::queue` spawns ffmpeg
   per item; a 200 ms `glib::timeout` polls progress and posts
   `Progress(item, pct)` to the model.
5. `Done` / `Failed(per_item_err)` — terminal per-item states.

Messages flow GTK widgets → model handlers → runtime jobs (off-thread)
→ `async_channel` back → GLib idle source → model → widget updates.
INVARIANTS.md Stage 6 documents the `Drop` impl that removes the
progress poll `SourceId` and the playback pump's `ControlFlow::Break`
exit when the sender is dropped.

## 4. Async boundaries

- **GTK main thread**: all widget mutation, model updates, timeouts.
- **`async_channel`** (single producer worker → consumer GLib idle):
  ffmpeg stderr lines, queue progress, playback ticks.
- **`gio::Cancellable`** paired with 30 s `glib::timeout_add_seconds_local_once`
  on the network-URL `gio::File.mount_*` path (INVARIANTS Stage 6 LEAK #2).
- **Blocking subprocess wait** is performed inside the runtime worker
  thread; never on the GTK thread.

There is no `tokio` runtime. Concurrency is GLib + standard threads.

## 5. External processes

All `Command::new(...)` invocations live in the shared
`big-media-runtime` / `big-converter-ui` crates; the audio binary itself
spawns nothing directly. Each invariant below is enforced today.

| Cmd | Caller | Argv invariant |
|---|---|---|
| `ffprobe` | `big-converter-ui/.../audio_edit_page/playback.rs:286` | `--` separator before path; no shell. |
| `ffmpeg` | `big-converter-ui/.../audio_edit_page/playback.rs:302` | explicit `-i <path>`; no shell. |
| `ffprobe` | `big-media-runtime/.../metadata_cache/{probe,full_info}.rs` | argv vector, path last after `--`. |
| `ffmpeg` | `big-media-runtime/converter/queue/processing.rs:369`, `converter/ffmpeg/command.rs:10`, `converter/subtitle.rs`, `converter/file_validator.rs` | argv vector; output path opened with `create_new`. |
| `ffmpeg`/`ffprobe` | `big-media-runtime/cast/{probe,transcode/*,gpu/ffmpeg}.rs` | scratch files inside owned 0700 `TempDir` per `TranscodeSession`. |
| `fd` | `big-media-runtime/media/playlist_scan/scan.rs:68` | argv vector; cwd-bound scan root. |
| `gio` | `big-media-runtime/media/playlist_scan/display.rs:{13,28,40}` | mount-list / restore; argv vector. |
| `lspci`, `vainfo`, `nvidia-smi` | `big-media-runtime/cast/gpu/detect.rs` | read-only probes; output parsed defensively. |
| `avahi-browse` | `big-media-runtime/cast/chromecast/network/avahi.rs:9` | argv vector. |

Global invariant: no `Command::new("sh"|"bash")`, no `shell=true`
equivalent, no untrusted-string interpolation. Path arguments are passed
as `OsStr` and (where flags precede them) guarded with `--` to prevent
leading-dash injection.

## 6. Security posture

See `INVARIANTS.md` (canonical). Highlights:

- **Stage 5 threat model.** GL symbol loader uses
  `dlopen("libGL.so.1", RTLD_LAZY|RTLD_LOCAL)` via `OnceLock`; no
  `RTLD_DEFAULT`. ffprobe/ffmpeg argv hardened with `--` and explicit `-i`.
- **`/tmp` race FIXED.** Scratch files for cast/transcode live in an
  owned 0700 `TempDir`; logs opened with `create_new(true).mode(0o600)`.
  Defended by `scratch_path_is_inside_owned_temp_dir_and_outside_tmp_world`.
- **Resource leaks FIXED** (Stage 6). Progress timer, GIO mount,
  playback pump all have explicit teardown paths.
- **Supply chain** — `deny.toml` ignores only `paste` 1.0.15 via lofty
  with rationale; `cargo-deny` runs in CI `supply-chain` job.
- **`#![forbid(unsafe_code)]` is *not* set** in this crate; unsafe
  blocks live in `big-converter-ui` (GL loader, narrow). Lints
  `not_unsafe_ptr_arg_deref = "deny"` and `unsafe_op_in_unsafe_fn = "warn"`
  guard the surface.

## 7. Testing strategy

- **Unit** — owned by `big-converter-ui` and `big-media-runtime`. The
  binary crate has no `src/*` business logic to unit-test.
- **Integration** — `tests/converter_integration.rs` links against the
  `lib.rs` re-exports plus `big-media-runtime` (dev-dep) to drive the
  shared queue end-to-end against fixture audio files.
- **Property / fuzz** — covered upstream in `big-media-runtime` for
  argv builders and probe parsers. None at this layer.
- **AT-SPI** — runs against the shared `big-converter-ui` widget tree;
  this binary inherits coverage via the launched application id
  `br.com.biglinux.AudioConverter`.
- **VM** — not required (no system services, no polkit, no DBus).
- **CI** — `cargo fmt`, `clippy --workspace --all-targets -D warnings`,
  `cargo deny check`, `cargo test --release --locked`.

## 8. Packaging

- **PKGBUILD** — `packaging/pacman/PKGBUILD`. Tarballs of this repo +
  `big-media-runtime` v2.0.0 + `big-rust-components` v0.1.0; symlinks
  them next to the source tree, patches `big-media-runtime/Cargo.toml`
  to point at the symlinked components path, then `cargo build --release
  --locked`. Installs `usr/bin/big-audio-converter`, the desktop file,
  AppStream metainfo, and a scalable SVG icon.
- **Install hook** — `packaging/pacman/big-audio-converter.install`.
- **Flatpak** — `packaging/flatpak/br.com.biglinux.AudioConverter.yml`.
- **Nix** — `packaging/nix/` (build-graph reproducibility).
- **AppStream** — `resources/metainfo/br.com.biglinux.AudioConverter.metainfo.xml`
  installed to `/usr/share/metainfo/`.
- **Desktop entry** — `resources/applications/br.com.biglinux.AudioConverter.desktop`.
- **Polkit** — none required. App runs as the user; ffmpeg/ffprobe run
  unprivileged.

### Next steps

- INVARIANTS.md — authoritative list of every defended property.
- `../big-converter-ui/` — UI + state.
- `../big-media-runtime/src/converter/` — ffmpeg orchestration.
