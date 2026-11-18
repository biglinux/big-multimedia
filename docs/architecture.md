# Workspace architecture

How the Big Multimedia workspace is split into crates, and where a change
belongs.

## Crates

```text
big-audio-player/            Package recipe (PKGBUILD) of the audio player
big-video-player/            Package recipe (PKGBUILD) of the video player
crates/
├── big-video-player/        Video player (binary)
├── big-audio-player/        Audio player (binary)
├── media-runtime/           big-media-runtime: mpv engine, casting, probing, settings, translations
├── foundation/              big-os-kit, big-audio-effects, big-media-engines, big-mpv-render, big-media-data
├── app/                     big-app-kit, big-app-kit-core: actions, MPRIS, dialogs, shortcuts
└── ui/
    ├── generic/big-relm4-components     Reusable Relm4/libadwaita components
    └── media/big-media-components       Media chrome, transport, metadata dialogs
        media/big-player-ui              Panels shared by the players
third_party/relm4/           Relm4 with a lifetime patch
```

Both players run on one playback engine, `big-media-runtime`, and one set of
interface components, so they look, sound and behave alike. The runtime's
module map and subprocess rules are in
[its architecture notes](../crates/media-runtime/ARCHITECTURE.md).

- Each player keeps its own `resources/` (desktop file, AppStream metadata,
  icon), `application-assets/` (GResource CSS) and `build.rs`.
- The only external BigLinux dependency is
  [big-gtk-kit](https://github.com/biglinux/big-gtk-kit), the shared GTK
  building blocks.
- The workspace `Cargo.toml`, `clippy.toml`, `deny.toml`,
  `rust-toolchain.toml` and `LICENSE` live at the root. `clippy.toml` bans GTK
  and libadwaita calls that leak or misbehave in the pinned versions;
  [Native library limits](native-library-limits.md) explains why.

## Conversion

Neither player converts media on its own. Cut, Convert and Extract Audio open
`big-video-converter-gui` or `big-audio-converter-gui` with the file and any
marked cuts (`--segments=START-END,...`), through
`big_media_runtime::shared::converter_handoff`.

## Translations

Both players share one gettext catalog in
[`crates/media-runtime/po`](../crates/media-runtime/po). Regenerate the
template after changing user-facing text:

```sh
crates/media-runtime/po/regen-pot.sh
```
