# big-media-runtime

GPL media engine shared by `big-audio-player` and `big-video-player`: the
libmpv backend and render paths, cast (BigNetScreen daemon client and the
optional built-in Chromecast sender), media probing and playlist scanning,
settings adapters, and the helpers both players share. The GTK-free data
services (SQLite library, settings store, radio/podcast clients) live in
`crates/foundation/big-media-data` and are re-exported here under their old
module paths.

Conversion is not part of this crate: `shared::converter_handoff` opens the
external converter GUIs with the file and the marked cuts.

## Features

| Feature | Pulls in |
|---|---|
| (default) | GTK- and mpv-free: data re-exports and GPU probing |
| `ui-core` | GTK/relm4/libmpv stack plus the modules both players need |
| `audio` | `ui-core` + SQLite music library and radio data |
| `video` | `ui-core` + mpv GL render pipeline (DMA-BUF offload, shaders) |
| `ui` | `audio` + `video` |
| `chromecast` | `ui-core` + built-in CASTV2 sender (`rust_cast`) |
| `fuzz` | slow property tests (`tests/filter_chain_props.rs`) |

## Resources and translations

`build.rs` compiles `data/resources` (video) and `data/audio-resources`
(audio) into GResource bundles exposed by `resources`. `po/` holds the single
gettext catalog for the whole workspace; regenerate the template with
`po/regen-pot.sh`.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the module map and the
subprocess rules.
