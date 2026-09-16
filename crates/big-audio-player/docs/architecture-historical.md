# HISTÓRICO — não usar como arquitetura vigente

Documento da base anterior, preservado em 16/09/2026. As versões, caminhos `window/imp` e a alegação de ausência de Relm4 abaixo estão desatualizados. Consulte `../ARCHITECTURE.md`.

# big-audio-player — Architecture

## 1. Purpose

GTK4/libadwaita audio player for BigLinux written in Rust (edition 2021,
toolchain 1.93, GPL-3.0-or-later). Single binary, single-crate project that
depends on the shared `big-media-runtime` (mpv/GStreamer playback runtime)
and the BigLinux UI kit (`big-app-kit`, `big-relm4-components`,
`big-media-components`). Features: local library + queue, playlists
(M3U/PLS, smart), browser (artist/album/genre/folder), radio (search,
favicons, explore cache), podcasts (search + feed fetch), lyrics panel,
album-art (file + MPRIS), Apple-Music-style detail view, podcasts queue,
preferences. Hands off transcoding to `big-audio-converter` /
`big-video-converter`.

## 2. Crate layout (real paths)

- `src/main.rs` — entrypoint; wires `App` + resources.
- `src/resources.rs` — compiled gresources loader.
- `src/audio/app.rs` — `adw::Application`.
- `src/audio/window/` — `imp::Window` (subclass) + `mod.rs`; state
  fields, `glib::SourceId`s, event sender.
- `src/audio/window/playback/` — `events.rs` (`PlayerEvent`),
  track/queue glue to `big-media-runtime`.
- `src/audio/window/library/`, `library_scan/`, `browser/`, `detail/`,
  `radio/`, `radio_search/`, `podcast.rs`, `podcast_feeds.rs`,
  `playlists/`, `playlists_io.rs`, `playlists_smart.rs`,
  `actions/`, `actions_setup/`, `dialogs.rs`, `detail_dialogs/`,
  `imp.rs`, `imp_close.rs`, `imp_constructed.rs`, `setup.rs`,
  `state.rs`, `theming.rs`, `views.rs`, `album_art.rs`,
  `smartphone.rs`.
- `src/audio/queue_view/` — queue widget + persistence.
- `src/audio/lyrics_panel.rs`, `popovers.rs`, `preferences.rs`,
  `keybindings.rs`, `appearance/`, `column_helpers.rs`,
  `library_grouping/`, `playlist_dialog/`, `playlist_panel/`,
  `browser_node.rs`.
- `src/audio/download_manager/` — podcast/episode downloads.
- `src/audio/converter_launcher.rs` — argv-safe spawn of external
  converter (only `Command::new` site).
- Shared workspaces (path deps): `big-media-runtime` (playback),
  `big-app-kit`, `big-relm4-components`, `big-media-components`.

## 3. State machine + message flow (GUI)

Not Relm4 — the window is a `glib::Object` subclass (`audio::window::imp`)
with `RefCell` fields. Event flow:

- Bootstrap: `audio::app::App::activate` → `Window::new` → `setup.rs`
  creates `async_channel::unbounded::<PlayerEvent>()`. The sender lives
  in `imp.event_sender: RefCell<Option<Sender<PlayerEvent>>>`; the
  receiver is consumed on a `glib::spawn_future_local` driver task.
- Player state: `Idle → Loading(track) → Playing → Paused → Ended →
  NextTrack` (loop), driven by `big-media-runtime` callbacks routed
  through `PlayerEvent`.
- Library scan: `Idle → Scanning(progress) → Indexing → Ready`;
  scan task lives off-thread, reports via the same event sender.
- Album art / lyrics / radio explore / podcast fetch: each is a
  `(Sender, Receiver)` pair on `async-channel`, the producer runs on
  `std::thread::spawn` or `gio::spawn_blocking`, the consumer on
  `glib::spawn_future_local`.
- Persistence: queue (`queue_view::persistence`), window state,
  playlists. Dirty-flag-coalesced flushes via timeout source.
- Close: `imp_close.rs` cancels every `glib::SourceId`, persists state,
  drops `event_sender`.

## 4. Async boundaries

- **GTK main loop (`glib::MainContext`).** All widget updates, all
  playback transitions, all `gio::Settings` writes.
- **`glib::spawn_future_local`.** Drives the `PlayerEvent` receiver and
  per-feature receivers (lyrics, album art, podcasts).
- **`gio::spawn_blocking`.** Used for: podcast search (`window/podcast.rs`),
  feed fetch (`podcast_feeds.rs`, `queue_view/persistence.rs`), thumbnail
  decode (`window/browser/thumbnail.rs`, `context_menu.rs`).
- **`std::thread::spawn`.** Album art fetch (`album_art.rs:43`), lyrics
  fetch (`lyrics_panel.rs:229`), feed parse (`podcast_feeds.rs:284`).
  Each hands results back via `async_channel::bounded(1)`.
- **`async-channel`.** Sole cross-thread signaling primitive. No
  `tokio` runtime is linked.
- **`glib::SourceId`.** Every reassignable timer field has a matching
  `.take().remove()` cancel (INVARIANTS.md §Stage 6).
- **`glib::clone!` discipline.** Zero `#[strong]` self captures
  (INVARIANTS.md §Stage 6).

## 5. External processes (`Command::new`)

- `src/audio/converter_launcher.rs:20` — `Command::new("big-audio-converter")`.
  Argv invariant: `.arg("--")` sentinel before any user-supplied path.
  Defended by INVARIANTS.md §Stage 5 H1.
- `src/audio/converter_launcher.rs:27` — fallback
  `Command::new("big-video-converter")` with identical `--` invariant.

No other `Command::new` callers exist. Playback uses
`big-media-runtime` (mpv/GStreamer) over FFI, not subprocess.

## 6. Security posture

- **Argv terminator.** `--` before user-path arg in `converter_launcher.rs`
  is the only subprocess boundary; INVARIANTS.md §Stage 5 H1 guards it
  with a grep regression.
- **Playlist size cap.** `MAX_PLAYLIST_BYTES = 16 MiB` in
  `src/audio/window/playlists/mod.rs:8`; defended by
  `import_rejects_oversize_playlist` + `import_accepts_small_playlist`.
- **Typed validators.** Playlist parsing, smart-playlist predicates,
  podcast URL parsing all reject malformed input before reaching IO.
- **No `/tmp` fallback.** Favicon, explore, MPRIS-art caches return
  `Option<PathBuf>` from `dirs::cache_dir`; callers early-return on
  `None` (INVARIANTS.md §Stage 6).
- **`SourceId` discipline + `#[weak]` captures.** No ref cycles, no
  hidden self-strong holds (INVARIANTS.md §Stage 6).
- **License-clean deps.** `cargo deny check` gate; one waiver
  (`RUSTSEC-2024-0436` paste via lofty) documented in `deny.toml`.
- **Atomic writes.** Settings + queue state via `big-app-kit`
  atomic-write helpers (consumed indirectly).
- **No polkit.** Not a privileged app.
- **`SAFETY` blocks.** Minimal `unsafe` in this crate; the FFI surface
  lives in `big-media-runtime`.

## 7. Testing strategy

- **Unit tests.** Inline `#[cfg(test)]` modules per module (playlist
  parser, smart-playlist filter, browser node ordering, lyrics matcher).
- **Integration tests.** Lightweight — most tests are inline.
- **Regression grep tests.** Argv-terminator regression for the
  converter spawn (INVARIANTS.md §Stage 5).
- **Performance budgets.** Stripped binary ≤ 25 MiB (current 18.08),
  click-to-play p50 ≤ 80 ms / p99 ≤ 300 ms, RSS budgets (INVARIANTS.md
  §Stage 7). Measured via `Makefile.ci`.
- **AT-SPI / a11y.** External validation through the `linux-ui-a11y`
  harness; no in-tree AT-SPI runner.
- **VM smoke.** `packaging/pacman/PKGBUILD` smoke build.
- **CI.** `.github/workflows/ci.yml` (deny + fmt + clippy + test +
  budgets).

## 8. Packaging

- **PKGBUILD.** `packaging/pacman/PKGBUILD` (Arch / Manjaro / BigLinux).
- **Flatpak.** `packaging/flatpak/` manifest (sandboxed runtime).
- **Nix.** `packaging/nix/` flake for reproducible builds.
- **Desktop file.** `resources/applications/br.com.biglinux.AudioPlayer.desktop`
  (also installed copy under `application-assets/desktop-integration/`).
- **AppStream metainfo.** `resources/metainfo/br.com.biglinux.AudioPlayer.metainfo.xml`.
- **Polkit.** None (no privileged actions).
- **i18n.** Strings route through `big_media_runtime::i18n`; gettext is
  initialized in `src/main.rs` via `shared::app_base::init_gettext()`. No
  `po/` catalog in this repo — translation catalogs live in
  `big-media-runtime`.
- **Build script.** `build.rs` compiles gresources at build time.
- **Optdeps.** `big-audio-converter` (preferred) or `big-video-converter`
  (fallback) for transcoding.
