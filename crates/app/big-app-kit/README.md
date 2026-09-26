# big-app-kit

GTK/libadwaita app helpers shared by `big-audio-player` and `big-video-player`.
GTK-free pieces live in `big-app-kit-core` and `big-os-kit` and are re-exported
here.

## Modules

- `actions`, `desktop` — GIO action/accelerator installers, file manager and
  power actions.
- `containment` — panic containment for Relm4 `update()` bodies and glib
  callbacks.
- `dialogs`, `file_dialogs` — dialog size persistence and GTK file dialogs.
- `keybindings` — shortcut parsing, overrides and accelerator labels.
- `mpris` — MPRIS2 D-Bus service.
- `widgets` — file drag-and-drop helpers.
- Re-exported: `files`, `recent_files`, `zone_layout`, `storage`,
  `subprocess`, `remote_control` (feature `remote-control`).

## Features

- `remote-control` — local HTTP remote-control endpoint.

## License

MIT
