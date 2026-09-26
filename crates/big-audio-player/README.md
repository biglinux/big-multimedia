# Big Audio Player

Music and audio player for BigLinux: library, playlists, queue, radio,
podcasts, lyrics and MPRIS, played through libmpv. A member of the
big-multimedia workspace; see the root [README](../../README.md).

The player converts nothing itself. Trim & Export and Convert hand the track
(and any marked cuts) to `big-audio-converter-gui` through
`big_media_runtime::shared::converter_handoff`.

## Layout

- `src/main.rs` calls `run()` in `src/lib.rs`; `lib.rs` also exposes
  `open_in_application` and `open_paths` for multicall hosts
  (big-host, big-modules).
- `src/audio/app.rs`: the `adw::Application` subclass (actions, accelerators,
  file-open broker).
- `src/audio/ui/audio_shell/`: the root Relm4 component that owns the mpv
  backend and routes every panel intent.
- `src/audio/ui/`: library, playlist, queue and dialog components.
- `src/audio/`: display-free engines (library query/scan, lyrics fetch,
  playlist import, queue undo, MPRIS state).
- `application-assets/resources/`: GResource CSS.
- `resources/`: desktop file, AppStream metadata and icon.
- `packaging/pacman/PKGBUILD`: builds this member from the workspace root.

## Build and run

```sh
cargo run --locked -p big-audio-player
```

## License

GPL-3.0-or-later (see the repository `LICENSE`).
