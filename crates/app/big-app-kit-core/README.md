# big-app-kit-core

The display-free half of `big-app-kit`: shortcut tables, recent files, file
names, the button-zone layout model and the LAN remote server.

## Entry points

- `keybindings`: `ShortcutDef` tables, `resolve_accels` and the parser of
  the `custom-keybindings` setting (`parse_custom`, `serialize_custom`).
- `recent_files::RecentStore`: the recent-files list kept between sessions.
- `zone_layout`: `ZoneLayoutSpec` (which controls exist and how the bars are
  laid out) and `ZoneLayout` (the controls in each zone, in order, and the
  hidden ones).
- `remote_control` (feature `remote-control`): the token-scoped HTTP server
  behind the phone remote.
- `files`: file-name sanitizing and display names.

## Where it fits

`big-app-kit` re-exports these modules unchanged and adds the GTK pieces (the
accelerator labels, dialogs, MPRIS, the tray). `big-relm4-components` uses
`zone_layout` and `keybindings` for its editors. The crate builds on
`big-os-kit` and links no GTK.

The LAN remote's limits and threat model are in
[Persistence and recovery](../../../docs/persistence-and-recovery.md#lan-remote).

## Validate a change

```sh
cargo test --locked -p big-app-kit-core --all-features
```

## License

MIT
