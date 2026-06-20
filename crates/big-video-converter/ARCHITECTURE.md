# ARCHITECTURE — big-video-converter

## 1. Purpose

`big-video-converter` is a GTK4/libadwaita desktop application for
transcoding video files via `ffmpeg`, with GPU detection (VA-API,
NVENC), an interactive edit page (crop preview, render loop on
`DrawingArea`), and a job queue. The UI lives in the shared
`big-converter-ui` + `big-media-runtime` crates. The shipped binary
(`src/main.rs`) is a **launcher**: it execs `big-host --separate
converter-video` so it shares the host-managed mpv engine + `big-convertd`
queue (video is `engine_owner`). `src/lib.rs::main_impl` keeps the
equivalent in-process boot (`big_converter_ui::run(RunOptions { mode: Video,
… })`) as the reference path. This differs from `big-audio-converter`, whose
binary runs the UI in-process — audio needs no native engine. INVARIANTS.md
notes that 47 of 52 source files in this twin are byte-identical with
`big-audio-converter`; this document calls out the video-only deltas.

The `dlopen` host module (`libbig_converter_video.so` + manifest) is owned by
`big-modules/big-converter-video` (package `big-converter-video`), not this repo.

## 2. Crate layout

| Crate | Path | Role |
|---|---|---|
| `big-video-converter` (bin+lib) | `src/main.rs`, `src/lib.rs` | Entry; selects `ConverterMode::Video`, no bootstrap env. |
| `big-converter-ui` | `../big-rust-components/crates/big-converter-ui` | Shared UI: window, file list, edit page, render preview, queue. |
| `big-media-runtime` (dev-dep) | `../big-media-runtime` | ffmpeg/ffprobe, GPU detect, cast/transcode sessions, thumbnails. |
| `gtk4` 0.11 (v4_22) | crates.io | GTK bindings. |

## 3. State machine and message flow

`ConverterModel` (video variant) is the single source of truth.

1. `Idle` — empty list.
2. `Editing` — files queued; user opens edit page per item. A
   GL-backed `DrawingArea` runs a 50 ms render-loop event pump
   (`edit_page/render.rs:409` — INVARIANTS Stage 6 marks this as a
   deferred follow-up; the audio twin's pump is already fixed).
3. `Queued` → `Converting` — queue worker spawns ffmpeg; 200 ms
   progress poll timer is owned by the model and removed in `Drop`.
4. `Done` / `Failed(per_item_err)`.

Crop/preview path: model sends `CropApply(rect)` → render loop reads
new state → next draw cycle shows the cropped frame. Budgeted at
p99 ≤ 200 ms (Stage 7).

Messages: GTK signal → model handler → off-thread runtime job →
`async_channel` → GLib idle → model → widget refresh.

## 4. Async boundaries

- **GTK main thread**: all widget + model mutation, GL context, render-loop tick.
- **`async_channel`**: queue worker → UI for ffmpeg progress and stderr lines.
- **`gio::Cancellable`** paired with 30 s timeout on network-URL mount (Stage 6).
- **Render loop**: GLib timeout, idempotent — does not spawn threads.
- **Subprocess wait**: blocking inside the runtime worker thread; never on GTK.
- No `tokio` runtime.

## 5. External processes

Identical surface to the audio twin, plus the video-only render and GPU paths.

| Cmd | Caller | Argv invariant |
|---|---|---|
| `ffprobe` | `big-converter-ui/.../audio_edit_page/playback.rs:286`; `big-media-runtime/.../{metadata_cache,cast,converter}/*` | `--` before path; argv vector. |
| `ffmpeg` | `big-converter-ui/.../audio_edit_page/playback.rs:302`; `big-media-runtime/.../converter/queue/processing.rs:369`, `converter/ffmpeg/command.rs:10`, `converter/subtitle.rs`, `converter/file_validator.rs`, `cast/transcode/session.rs:{141,254}`, `cast/gpu/ffmpeg.rs:{6,19}`, `player/thumbnail.rs:130`, `media/thumbnail.rs:111` | explicit `-i`; argv vector; scratch dir is owned 0700 `TempDir`. |
| `fd` | `big-media-runtime/media/playlist_scan/scan.rs:68` | argv vector; bounded root. |
| `gio` | `big-media-runtime/media/playlist_scan/display.rs:{13,28,40}` | argv vector. |
| `vainfo` | `big-media-runtime/cast/gpu/detect.rs:10` | read-only GPU probe; defensive parse. |
| `lspci` | `cast/gpu/detect.rs:49` | argv vector. |
| `nvidia-smi` | `cast/gpu/detect.rs:73` | argv vector; exit-status branch. |
| `avahi-browse` | `cast/chromecast/network/avahi.rs:9` | argv vector. |

Global invariant: no shell interpolation; no `sh -c`; all paths are
`OsStr`; leading-dash defense via `--`.

## 6. Security posture

See `INVARIANTS.md`. Highlights:

- **GL symbol loader (video-critical)** — `dlopen("libGL.so.1",
  RTLD_LAZY|RTLD_LOCAL)` cached in `OnceLock`; cannot be hijacked by
  `LD_PRELOAD` symbol resolution. Located in
  `big-converter-ui/src/converter/ui/edit_page/helpers.rs`.
- **ffmpeg / ffprobe argv hardened** — `-i` explicit, `--` separator.
- **Cast transcode scratch** — owned 0700 `TempDir`, `create_new` +
  mode 0600 on log files.
- **Resource lifecycle** — progress timer, GIO mount cancellable,
  playback pump teardown all defended; the video-only render-loop
  pump is the one tracked open follow-up.
- **Supply chain** — `deny.toml` matches the audio twin; only `paste`
  1.0.15 ignored.
- **DrawingArea preview is intentionally non-focusable** (Stage 9);
  interactive controls live in the overlay strip (Tab-navigable).
- **`#![forbid(unsafe_code)]` not set** at the binary level; unsafe
  is scoped to the GL loader in `big-converter-ui`.

## 7. Testing strategy

- **Unit** — in `big-converter-ui` (model, widget builders) and
  `big-media-runtime` (argv builders, probe parsers).
- **Integration** — `tests/converter_integration.rs`: drives the
  shared queue against fixture clips, asserts ffmpeg argv shape and
  output existence; uses `big-media-runtime` as dev-dep.
- **Property** — argv builders and crop-rect math (upstream).
- **Fuzz** — probe-output parser fuzz target (upstream in
  `big-media-runtime`).
- **AT-SPI** — shared `big-converter-ui` smoke; this binary inherits
  via the launched app id `br.com.biglinux.VideoConverter`.
- **VM** — not required (no system service, no polkit).
- **CI** — fmt, clippy `-D warnings`, deny, tests (nextest, falling back to
  `cargo test`). `Cargo.lock` is committed but CI does not yet pass `--locked`;
  the full local gate is in `REVIEW.md`.

## 8. Packaging

- **PKGBUILD** — `packaging/pacman/PKGBUILD`. Same recipe as the audio
  twin: pulls tarballs of this repo + `big-media-runtime` v2.0.0 +
  `big-rust-components` v0.1.0, symlinks them, patches the runtime's
  `Cargo.toml` to the local components path, then
  `cargo build --release --locked`. Installs binary, desktop, metainfo,
  scalable SVG icon. `depends=(gtk4 libadwaita mpv ffmpeg)`.
- **Install hook** — `packaging/pacman/big-video-converter.install`.
- **Flatpak** — `packaging/flatpak/br.com.biglinux.VideoConverter.yml`.
- **Nix** — `packaging/nix/`.
- **AppStream** — `resources/metainfo/br.com.biglinux.VideoConverter.metainfo.xml`.
- **Desktop entry** — `resources/applications/br.com.biglinux.VideoConverter.desktop`.
- **Polkit** — none. Runs unprivileged; GPU detect probes only.

### Next steps

- INVARIANTS.md — authoritative invariant list and audio/video deltas.
- `../big-rust-components/crates/big-converter-ui/src/converter/ui/edit_page/render.rs` — video render loop.
- `../big-media-runtime/src/cast/gpu/` — VA-API / NVENC detection.
- `../big-media-runtime/src/converter/` — ffmpeg orchestration.
