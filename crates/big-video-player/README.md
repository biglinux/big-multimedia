# Big Video Player

Hardware-accelerated video player for BigLinux, built on libmpv with a GL
render path: local files and folders, IPTV, online video, subtitles, picture
adjustments, MPRIS and (feature `chromecast`) casting. A member of the
big-multimedia workspace; see the root [README](../../README.md).

The player converts nothing itself. Cut Video, Convert and Extract Audio hand
the file (and any marked cuts) to `big-video-converter-gui` or
`big-audio-converter-gui` through
`big_media_runtime::shared::converter_handoff`.

## Layout

- `src/main.rs` calls `run()` in `src/lib.rs`; `lib.rs` also exposes
  `open_in_application`, `open_paths` and `mount_as_background_pane` for
  multicall hosts (big-host, big-modules).
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
cargo run --locked -p big-video-player --features chromecast
```

## License

GPL-3.0-or-later (see the repository `LICENSE`).
