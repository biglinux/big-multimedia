# Architecture — big-media-runtime

## Modules

Everything except `cast::gpu` and the `big-media-data` re-exports is behind a
feature (see the README).

- `player/` — `mpv_backend/` (libmpv client, properties, audio filter chain,
  screenshots, GL/EGL render paths: `fast_render`, `offload_renderer`,
  `egl_dmabuf`, `software_frames`, `video_surface`), `queue.rs` (audio play
  queue), `thumbnail.rs` (seek-bar hover frames via ffmpeg).
- `cast/` — `gpu/` (encoder probing), `probe.rs` + `transcode/` (ffprobe and
  the live ffmpeg transcode), `sandbox.rs` (bubblewrap wrapper for every cast
  ffmpeg/ffprobe), `daemon.rs` (D-Bus client for the BigNetScreen cast
  daemon), and `chromecast/` (feature `chromecast`: avahi discovery, CASTV2
  control and the token-gated HTTP stream server).
- `media/` — playlist parsing and folder scans (`playlist_scan/`), IPTV M3U
  (`iptv/`), ffprobe metadata cache, native MP4/MKV duration readers, fuzzy
  search, thumbnails.
- `settings/` — re-exports `big_media_data::settings` plus the GTK adapters:
  dialog-size persistence, the acknowledged write queue (`saving.rs`) and the
  msgid anchors for dynamically chosen status text (`messages.rs`).
- `shared/` — code both players use: app boot (`app_base`), audio effects and
  equalizer, `converter_handoff`, dialogs, popovers, preference rows,
  shortcut capture, sleep timer, smartphone remote, theming, tooltips,
  illustrations.
- `recent.rs`, `resources.rs` — recent-file store and the compiled GResource
  bundles.

Dependencies point one way: this crate uses `big-media-data`,
`big-os-kit`, `big-app-kit`, `big-relm4-components`, `big-media-components`,
`big-audio-effects` and `big-mpv-render`; none of them depends on it.

## Threads

- The GTK main loop drives the players; there is no async runtime.
- libmpv events are read on a dedicated thread and forwarded to the UI through
  `async-channel`.
- Every subprocess and blocking probe runs off the GTK thread
  (`gio::spawn_blocking` or a worker thread).

## Subprocesses

All go through `big_os_kit::subprocess::BigSubprocessSpec` (argv arrays, no
shell) except the two `bwrap` launches that need raw `Command` pipes.

| Site | Program |
|---|---|
| `player/thumbnail.rs`, `media/thumbnail.rs` | `ffmpeg`, `ffprobe` |
| `media/metadata_cache/` | `ffprobe` |
| `media/playlist_scan/scan.rs` | `fd` |
| `media/playlist_scan/display.rs` | `gio` |
| `cast/gpu/` | `ffmpeg -encoders/-hwaccels`, `lspci` |
| `cast/probe.rs`, `cast/transcode/` | `ffprobe`/`ffmpeg` inside `bwrap` |
| `cast/chromecast/network/avahi.rs` | `avahi-browse` |
| `shared/converter_handoff.rs` | `big-{video,audio}-converter-gui` |

Program names are fixed strings. User paths are passed as separate argv
elements, after `--` where the tool accepts it.

## Security notes

- `unsafe` is limited to the libmpv/EGL/GL FFI and a few libc calls in
  `shared/app_base.rs`; `unsafe_op_in_unsafe_fn` and
  `not_unsafe_ptr_arg_deref` are enforced in `Cargo.toml`.
- Cast ffmpeg/ffprobe run in bubblewrap with no network and only the input
  files, an owned `0700` scratch dir (`tempfile`) and the GPU render nodes.
- The Chromecast HTTP server binds `0.0.0.0` (the receiver must reach it) and
  serves only requests carrying a 128-bit random token; without
  `/dev/urandom` it refuses to start.
- SQL goes through parameterized queries in `big-media-data`;
  `tests/sql_injection.rs` is the regression test.

## Tests

Unit tests live next to the code. `tests/sql_injection.rs` needs `ui`;
`tests/filter_chain_props.rs` (proptest) needs `ui` and `fuzz`. Tests that
need a real mpv audio output or the BigNetScreen daemon are `#[ignore]`d and
state the environment they expect.
