# Architecture — big-media-runtime

> **Reorganization in progress (R1–R4).** This crate is being split by
> concern: GTK-free data services (db, settings, queue, library, radio,
> download, media, recent) move to a new sibling crate `big-media-data`;
> the reusable Relm4 player panels (`player_ui`) move to
> `big-rust-components/crates/big-player-ui`; this crate slims to **engines
> only** (mpv player, cast, camera, mpris).
> Placement rule + target graph: `big-rust-components/ARCHITECTURE.md` §9–10.
> Each phase keeps `pub use` shims so consumers never break mid-migration.

## 1. Purpose

`big-media-runtime` is the shared library that backs every BigLinux media
player (`big-audio-player`, `big-video-player`). It owns the mpv-driven
playback core, the SQLite music library, Chromecast discovery and HTTP
streaming, MPRIS D-Bus integration, settings persistence, and app-facing
runtime adapters. The crate is a single
library; per-app binaries live in sibling repositories that consume it.

## 2. Crate Layout

Single workspace package, `name = "big-media-runtime"`, `lib name =
big_media_runtime`. Modules under `src/`:

- `cast/` — Cast-oriented helpers: `gpu/` detection (VAAPI/NVENC),
  `probe.rs` ffprobe wrappers, `transcode/` sessions, and optional
  `chromecast/network/avahi.rs` discovery behind the `chromecast` feature.
- `db/` — SQLite music library: `schema.rs`, `queries.rs`,
  `queries_advanced/`, `fts.rs`, `models.rs` (rusqlite).
- `library/` — Library scanner + online metadata: `scanner.rs` (ignore
  walk), `metadata.rs` (lofty), `lastfm.rs`, `musicbrainz.rs`,
  `listenbrainz.rs`, `lrclib.rs`, `coverart.rs`, `podcast.rs`.
- `media/` — Helpers: `playlist.rs`, `playlist_scan/` (fd/gio), `iptv/`,
  `thumbnail.rs`, `metadata_cache/` (ffprobe-backed), `fuzzy_search.rs`,
  `native_duration.rs`.
- `player/` — `mpv_backend/`, `queue.rs`, `thumbnail.rs` (ffmpeg).
- `mpris/` — D-Bus media player iface (zbus-style hand-roll).
- `settings/` — Persistent key/value store: `store.rs`, `keys.rs`,
  `defaults.rs`, `dialog.rs`, `tests.rs`.
- `shared/` — Temporary app-facing adapters while R4 completes:
  `app_base.rs`, `audio_effects.rs`, `audio_effects_eq.rs`,
  `converter_handoff.rs` (opens the converter GUIs with marked cuts), `popovers.rs`,
  `preference_rows.rs`, `shortcut_capture.rs`, `sleep_timer.rs`,
  `smartphone/`, `theming.rs`, `tooltip.rs`, `ui_helpers.rs`.
  Player appearance and audio preference pages live in
  `big-rust-components/crates/big-player-ui`.
- `config.rs`, `error.rs`, `i18n.rs`, `recent.rs`, `resources.rs`,
  `fuzz_entry.rs` (feature `fuzzing`).

Upstream dependency direction: `big-audio-effects`, `big-app-kit`,
`big-relm4-components`, `big-media-components`, `big-mpv-render` (all from
`../big-rust-components/crates/*`). No reverse deps.

## 3. Data Flow Between Modules

```
                user input (per-app Relm4 model)
                          |
                          v
   shared/ adapters + big-player-ui pages
                          |
        +-------------------------+
        v                         v
    player/                     cast/
    mpv_backend                 transcode
        |                         | ffprobe + ffmpeg
        v                         v
       mpv                  tiny_http server -> Chromecast
        |
        +--> mpris/ (D-Bus notifications)
        +--> recent.rs (XDG state)
        +--> db/ (library lookups, FTS5)

   library/scanner --(metadata via lofty)--> db/
                    --(online lookup via big-os-kit/system curl)--> lastfm / mb / listenbrainz / lrclib
```

Cast session state: `Discovered -> Probed (codecs) -> Selected (direct |
transcode) -> Streaming -> Stopped`. Transcoded paths spawn ffmpeg with
output piped into the local HTTP server (`tiny_http`) on a port advertised
to the Chromecast over `rust_cast`. This path is only compiled with the
`chromecast` feature; default `ui` builds keep Cast probing/transcode helpers
without linking CASTV2/TLS/protobuf.

## 4. Async Boundaries

- GTK main loop drives Relm4 components; runtime exposes no separate tokio
  runtime.
- `async-channel` (multi-producer, single-consumer) carries cast
  progress events from worker threads to UI components.
- `big_os_kit::http_client` calls run on dedicated `std::thread::spawn`
  workers (blocking system `curl` I/O); results posted back via
  `async-channel`.
- Optional `avahi-browse` Cast discovery and `dns-lookup` reverse-DNS run on
  their own threads.
- ffmpeg / ffprobe / vainfo / lspci / nvidia-smi / avahi-browse / fd / gio
  invocations always use `std::process::Command` with explicit
  `stdout`/`stderr` capture; never on the GTK thread.
- `mpv` callbacks are routed through the libmpv2 event thread and bridged
  to GTK via `glib::MainContext::invoke`.

## 5. External Processes

Every `Command::new` site + argv invariant:

| Site | Argv invariant |
|------|----------------|
| `player/thumbnail.rs:130` `ffmpeg` | `-ss <sec> -i <abs path> -frames:v 1 -vf scale=… <abs out>`. Input is canonicalized; output is `O_TMPFILE`-friendly path under XDG cache. |
| `media/thumbnail.rs:86` `ffprobe` | `-v error -show_entries format=duration -of default=… <abs path>`. Static flags. |
| `media/thumbnail.rs:111` `ffmpeg` | Same as above; thumbnail extraction. |
| `media/playlist_scan/display.rs:13,28,40` `gio` | `info` / `list` / restore — argv built from static strings + sanitized URI. |
| `media/playlist_scan/scan.rs:68` `fd` | Patterns + `--type f --extension` static; root path canonicalized. |
| `media/metadata_cache/probe.rs:132`, `media/metadata_cache/full_info.rs:23` `ffprobe` | `-v error -print_format json -show_format -show_streams <abs path>`. |
| `cast/probe.rs:{26,53,68}`, `cast/transcode/probe.rs:29` `ffprobe` | Codec / format introspection. |
| `cast/gpu/detect.rs:{10,49,73}` `vainfo`, `lspci`, `nvidia-smi` | Hardware capability probe. Static argv only; output parsed as text. |
| `cast/gpu/ffmpeg.rs:{6,19}` `ffmpeg` | Encoder-list probes (`-encoders`, `-hwaccels`). Static argv. |
| `cast/transcode/session.rs:{141,254}` `ffmpeg` | Live transcode. Output URL is `pipe:1` into the local `tiny_http` server. |
| `cast/chromecast/network/avahi.rs:9` `avahi-browse` | `-r -t -p _googlecast._tcp` static argv. |
| `shared/converter_handoff.rs` `big-{video,audio}-converter-gui` | Fixed program names; `--segments=START-END,...` from player state, then `--` and the file paths. |

Invariants:
- No `Command::new` ever sees a user-controlled binary name; all are
  fixed-string system tools or the fixed converter GUI names.
- Path arguments are always `&Path` / `&OsStr`, never formatted into a
  shell string.
- Subtitle stream indices and probe-derived integers are validated before
  being placed in argv.

## 6. Security Posture

- `unsafe` is scoped: the mpv/EGL/GL FFI boundary plus a small set of libc
  calls (`libc::setlocale`
  and `std::env::set_var` in `shared/app_base.rs`). The crate does **not**
  `#![forbid(unsafe_code)]`. Gated by `unsafe_op_in_unsafe_fn = "warn"` and
  `not_unsafe_ptr_arg_deref = "deny"` (clippy lints in `Cargo.toml`).
- Cast transcode uses `tempfile` to create owned, mode-`0700` scratch dirs;
  defeats `/tmp` symlink races (documented in `Cargo.toml`).
- SQL: all queries are parameterized; regression test in
  `tests/sql_injection.rs`.
- HTTP: metadata calls go through `big_os_kit::http_client`, which delegates
  TLS/CA handling to the system `curl` binary; no plain-HTTP fallback for
  metadata calls.
- mDNS: Cast discovery delegates to the system Avahi daemon through fixed
  `avahi-browse` argv; no embedded mDNS stack is linked into the runtime.
- `tiny_http` cast server binds to a loopback-only / link-local port and
  serves only the active transcode session (token-gated path).
- Fuzz feature (`fuzzing`) exposes `fuzz_entry::*` wrappers around
  parsers (subtitle, playlist, IPTV). Never enabled in release.

## 7. Testing Strategy

- Unit: per-module tests (e.g., `settings/tests.rs`).
- Property: `tests/filter_chain_props.rs` (proptest) for ffmpeg filter
  composition; gated by `fuzz` feature for slow cases.
- Security regression: `tests/sql_injection.rs`.
- Fuzz: feature-gated `fuzz_entry` consumed by sibling `fuzz/` crates in
  the parent workspace.
- Lints: `clippy::pedantic` (with documented allow-list in `Cargo.toml`)
  + `unsafe_op_in_unsafe_fn` enforced.
- No GTK widget runtime tests in this crate; UI tests live in the
  per-app consumer repos.

## 8. Packaging (Workspace Consumption)

`big-media-runtime` is **not** a standalone package. It is consumed as a
path dependency by:

- `big-audio-player`, `big-video-player` (BigLinux media players)

Each consuming app's PKGBUILD declares the system-level runtime deps that
this crate's external processes require: `ffmpeg`, `mpv` (libmpv ≥ render
API), `avahi`, `mesa-utils` (vainfo), `pciutils` (lspci), `fd`,
`glib2`/`gio`, `sqlite`, `dconf`. Static resources (`data/`,
`po/messages.mo`) ship with the crate and are bundled via
`gio-resource` at build time (see `build.rs`). Apps that consume this
crate inherit GResource registration through
`big_media_runtime::resources::register()`.
