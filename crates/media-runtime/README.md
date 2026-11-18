# big-media-runtime

GPL media engine shared by `big-audio-player` and `big-video-player`: libmpv
playback and rendering, casting, media probing and the helpers both players use.

It holds the libmpv backend and render paths, the client of the BigNetScreen
cast daemon, media probing and playlist scanning, and the settings adapters.
The GTK-free data services (SQLite library, settings store, radio/podcast
clients) live in `crates/foundation/big-media-data` and are re-exported here as `config`, `db`, `error`, `i18n`, `library` and `settings`.

Conversion is not part of this crate: `shared::converter_handoff` opens the
external converter GUIs with the file and the marked cuts.

## Features

| Feature | Pulls in |
|---|---|
| (default) | GTK- and mpv-free: the `big-media-data` re-exports only |
| `ui-core` | GTK/relm4/libmpv stack plus the modules both players need |
| `audio` | `ui-core` + SQLite music library and radio data |
| `video` | `ui-core` + mpv GL render pipeline (DMA-BUF offload, shaders) |
| `ui` | `audio` + `video` |
| `fuzz` | slow property tests (`tests/filter_chain_props.rs`) |

## Resources and translations

`build.rs` compiles `data/audio-resources` (the audio player's sleep-timer
illustrations) into the GResource bundle exposed by `resources`. `po/` holds
the single gettext catalog for the whole workspace; regenerate the template
with `po/regen-pot.sh`.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the module map and the
subprocess rules.

## Validate a change

```sh
cargo test --locked -p big-media-runtime --all-features
```

The `#[ignore]`d tests need a real mpv audio output or the BigNetScreen daemon;
[ARCHITECTURE.md](ARCHITECTURE.md#tests) lists them.
