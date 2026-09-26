# big-relm4-components

Reusable Relm4/libadwaita components shared by `big-audio-player` and
`big-video-player`.

The crate wraps libadwaita through Relm4/gtk-rs. It does not copy or vendor
libadwaita implementation code, CSS, screenshots, icons, or demo assets.
Before adding a component, check whether `relm4`, `relm4-components` or a
GTK/libadwaita widget already solves it.

## Modules

- `display` — sleep timer dialog and texture conversion.
- `feedback` — context/hover popovers and the custom tooltip.
- `input` — slider, dropdown and spin rows, shortcut capture/dialog, and the
  zone-layout editor.
- `layout` — media window chrome and sidebar, dialogs, hamburger menu, toolbar
  panes, zone applier.
- `list` — `AdwActionRow` derivatives.
- `task` — off-main-thread work with main-thread delivery.
- `text`, `time`, `theme`, `i18n` — small shared helpers.

## License

MIT
