# big-app-kit

GTK/libadwaita app helpers shared by `big-audio-player` and `big-video-player`.
GTK-free pieces live in `big-app-kit-core` and `big-os-kit` and are re-exported
here.

## Modules

- `actions`, `desktop` — GIO action/accelerator installers, file manager and
  power actions.
- `containment` — panic containment for Relm4 `update()` bodies and glib
  callbacks.
- `file_dialogs` — GTK file dialogs.
- `keybindings` — shortcut parsing, overrides and accelerator labels.
- `mpris` — MPRIS2 D-Bus service.
- `tray` — system tray icon (StatusNotifierItem) with a menu.
- `widgets` — file drag-and-drop helpers.
- Re-exported: `files`, `recent_files`, `zone_layout`, `storage`,
  `subprocess`, `remote_control` (feature `remote-control`).

## Features

- `remote-control` — local HTTP remote-control endpoint.

## Validate a change

```sh
cargo test --locked -p big-app-kit --all-features
```

The `#[ignore]`d tray test needs a session bus of its own;
[CONTRIBUTING](../../../CONTRIBUTING.md#build-and-check) gives the command.

## License

MIT
