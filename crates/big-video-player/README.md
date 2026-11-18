# Big Video Player

Hardware-accelerated video player for BigLinux, built on libmpv with a GL
render path. It plays local files and folders, IPTV and online video, with
subtitles, picture adjustments, MPRIS and playback on a TV through the
BigNetScreen daemon. A member of the big-multimedia workspace; see the root
[README](../../README.md).

The player converts nothing itself. Cut Video, Convert and Extract Audio hand
the file (and any marked cuts) to `big-video-converter-gui` or
`big-audio-converter-gui` through
`big_media_runtime::shared::converter_handoff`.

## Command line

```sh
big-video-player [--new-window] [FILE|URI...]
```

- One local file plays and puts its folder in the sidebar; playback continues
  through that folder in the sidebar's sort order.
- One playlist file (`.m3u`, `.m3u8`, `.pls`) plays its entries.
- One URL plays the stream. The schemes are `http`, `https`, `rtsp`, `rtsps`,
  `rtmp`, `rtmps`, `udp`, `rtp`, `srt`, `mms`, `dvd` and `bd`.
- Several arguments play the media files among them, in the order given.
- Without `--new-window`, a second launch hands its files to the window already
  open, or raises it, and exits. `--new-window` starts a separate player instead.

`--help` lists the options; `--help-gapplication` adds GLib's
`--gapplication-service`, which only D-Bus activation uses.

| Exit status | Meaning |
|---|---|
| 0 | The player ran and closed, or handed its files to the running player |
| 1 | GLib refused the command line, such as an unknown option, and printed why |

## Layout

- `src/main.rs` calls `run()` in `src/lib.rs`; `lib.rs` also exposes
  `open_in_application` and `open_paths` for multicall hosts (big-host,
  big-modules).
- `src/video/app.rs`: the `adw::Application` subclass (actions, accelerators,
  file-open broker).
- `src/video/ui/video_shell/`: the root Relm4 component that owns the mpv
  backend, the render surface, MPRIS and the cut marks (`trim/`).
- `src/video/ui/`: controls bar, browser sidebar, preferences, appearance,
  filters, IPTV and cast components.
- `application-assets/resources/`: GResource CSS.
- `resources/`: desktop file, AppStream metadata and icon.

## Build and run

```sh
cargo run --locked -p big-video-player
```

## License

GPL-3.0-or-later (see the repository `LICENSE`).
