# Architecture: big-media-runtime

## Modules

Everything except the `big-media-data` re-exports is behind a feature (see the
README).

- `player/`: `mpv_backend/` (libmpv client, properties, audio filter chain,
  screenshots, GL/EGL render paths: `fast_render`, `offload_renderer`,
  `egl_dmabuf`, `software_frames`, `video_surface`), `queue.rs` (audio play
  queue), `thumbnail.rs` (seek-bar hover frames via ffmpeg).
- `cast/`: `daemon.rs`, the D-Bus client of BigNetScreen, which owns
  Chromecast/DLNA discovery, streaming and conversion.
- `media/`: playlist parsing and folder scans (`playlist_scan/`), IPTV M3U
  and provider store (`iptv/`), ffprobe metadata cache, native MP4/MKV
  duration readers, fuzzy search, thumbnails.
- `settings/`: re-exports `big_media_data::settings` plus the GTK adapters:
  dialog-size persistence, save status and close draining (`saving.rs`) and
  the msgid anchors for dynamically chosen status text (`messages.rs`).
- `shared/`: code both players use: app boot (`app_base`), audio effects and
  equalizer, `converter_handoff`, popovers, preference rows, shortcut
  capture, sleep timer, theming, illustrations, and the "Play on a TV" page
  and card (`tv_cast`).
- `recent.rs`, `resources.rs`: recent-file store and the compiled GResource
  bundles.

Dependencies point one way: this crate uses `big-media-data`,
`big-os-kit`, `big-app-kit`, `big-gtk-kit`, `big-relm4-components`,
`big-media-components`, `big-audio-effects` and `big-mpv-render`; none of
them depends on it.

## Threads

- The GTK main loop drives the players; there is no async runtime.
- libmpv events are read on a dedicated thread, woken by mpv's wakeup
  callback, and forwarded to the main loop.
- Every subprocess and blocking probe runs off the GTK thread
  (`gio::spawn_blocking` or a worker thread).

## Caches and state

- The metadata cache (`metadata_cache.json`) and generated thumbnails live
  under `$XDG_CACHE_HOME`; recent files under `$XDG_STATE_HOME`. Without
  those directories (no `HOME`) nothing is written: the metadata cache stays
  in memory, thumbnails are not generated and no recent files are kept.
- Cache and state files are replaced by an atomic rename without `fsync`; a
  thumbnail is rendered to a hidden sibling and renamed into place.

## Subprocesses

All go through `big_os_kit::subprocess::BigSubprocessSpec` (argv arrays, no
shell).

| Site | Program |
|---|---|
| `player/thumbnail.rs`, `media/thumbnail.rs` | `ffmpeg`, `ffprobe` |
| `player/mpv_backend/backend/` | `ffmpeg`, `pactl` |
| `media/metadata_cache/` | `ffprobe` |
| `media/playlist_scan/scan.rs` | `fd` |
| `media/playlist_scan/display.rs` | `gio trash` |
| `shared/converter_handoff.rs` | `big-{video,audio}-converter-gui` |

Program names are fixed strings. User paths are passed as separate argv
elements, after `--` where the tool accepts it.

## Security notes

- `unsafe` is limited to the libmpv/EGL/GL FFI and the bootstrap environment
  and locale calls in `shared/app_base.rs`; `unsafe_op_in_unsafe_fn` and
  `not_unsafe_ptr_arg_deref` are enforced in `Cargo.toml`.
- Playlists are read up to 64 MiB; entries that resolve under `/etc`,
  `/proc`, `/sys`, `/dev`, `/root` or `/boot` are dropped.
- Strings from the cast daemon are validated against the expected D-Bus types
  and escaped before they reach markup.
- SQL goes through parameterized queries in `big-media-data`;
  `tests/sql_injection.rs` is the regression test.

## Tests

Unit tests live next to the code. `tests/sql_injection.rs` needs `ui`;
`tests/filter_chain_props.rs` (proptest) needs `ui` and `fuzz`. Tests that
need a real mpv audio output or the BigNetScreen daemon are `#[ignore]`d and
state the environment they expect.
