# HISTÓRICO — não usar como arquitetura vigente

Documento da base anterior, preservado em 16/09/2026. As versões, caminhos `window/imp` e a alegação de ausência de Relm4 abaixo estão desatualizados. Consulte `../ARCHITECTURE.md`.

# big-video-player — Architecture

## 1. Purpose

GTK4/libadwaita video player for BigLinux written in Rust (edition 2021,
toolchain 1.93, GPL-3.0-or-later). Single binary, single-crate. Wraps the
shared `big-mpv-render` (libmpv + OpenGL render) and `big-media-runtime`
runtime, plus BigLinux UI kit (`big-app-kit`, `big-relm4-components`,
`big-media-components`). Features: local files, IPTV/M3U browser with
channel logos, yt-dlp paste-and-play with format selection, image
adjustment (brightness/contrast/saturation/gamma), audio/video delay,
sleep timer, sidebar, on-screen controls, screenshot via ffmpeg, cast
device discovery (mDNS) with sync overlay, KDE-aware keep-above.

## 2. Crate layout (real paths)

- `src/main.rs` — entrypoint.
- `src/resources.rs` — gresources loader.
- `src/video/app.rs` — `adw::Application`.
- `src/video/mod.rs`, `video_options.rs`, `keybindings.rs`,
  `controls.rs`, `image_adjust.rs`, `delay_adjust.rs`, `sleep_timer.rs`,
  `now_playing.rs`.
- `src/video/appearance/` — theme + style.
- `src/video/sidebar/`.
- `src/video/window/` — `imp::Window` subclass + `mod.rs`.
- `src/video/window/init/` — `mpv.rs` (player bootstrap), `controls.rs`,
  `lifecycle.rs` (setup_periodic_flush), `header bar wiring`.
- `src/video/window/playback/events.rs` — `PlayerEvent`.
- `src/video/window/overlays/screenshot.rs` — ffmpeg screenshot path.
- `src/video/window/overlays_cast/sync.rs`, `cast_bridge.rs` — cast
  device control thread bridge.
- `src/video/dialogs/` — `format_selection.rs` (yt-dlp probe),
  `cast/device_dialog.rs` (mDNS probe).
- `src/video/iptv_view/` — `browser.rs`, `logos.rs` (favicon fetch).
- `src/video/preferences/`.
- Shared workspaces (path deps): `big-media-runtime`, `big-mpv-render`,
  `big-app-kit`, `big-relm4-components`, `big-media-components`.

## 3. State machine + message flow (GUI)

`adw::ApplicationWindow` subclass with `RefCell` fields. Like
big-audio-player, this is not Relm4.

- Bootstrap: `app.rs` → `Window::new` → `init/mpv.rs` constructs the
  mpv render context, hooks events into `playback/events.rs`. An
  `async_channel::Sender<PlayerEvent>` lives in
  `imp::Window.event_sender` (`window/mod.rs:162`).
- Player state: `Idle → Loading → Buffering → Playing → Paused →
  Seeking → EndOfFile`, driven by mpv property observers.
- yt-dlp paste-and-play: `Idle → Probing(URL) → FormatList(shown) →
  Selected → Playing`. Probe runs on `gio::spawn_blocking`
  (`format_selection.rs:108`) and unblocks the UI thread.
- Cast: `NoDevices → Scanning → DeviceList → Connected(device) →
  Streaming`. Scan and sync run on `std::thread::spawn`
  (`overlays_cast/sync.rs:29,71`, `cast_bridge.rs:16,76`); results are
  marshalled back via `async-channel`.
- IPTV: `Empty → Importing(M3U) → Ready → SelectedChannel → Playing`,
  with logo fetch off-thread (`iptv_view/logos.rs`).
- Lifecycle: `init/lifecycle.rs::setup_periodic_flush` uses
  `glib::timeout_add_seconds_local` + `#[weak] self` +
  `ControlFlow::Break` fallback for auto-cancel on window finalize.

## 4. Async boundaries

- **GTK main loop.** All mpv property writes, widget updates,
  persistence flips.
- **`glib::spawn_future_local`.** Drives `PlayerEvent` receiver and
  per-feature receivers (yt-dlp probe, mDNS scan, IPTV logos).
- **`gio::spawn_blocking`.** yt-dlp probe (`format_selection.rs:108`),
  mDNS probe (`dialogs/cast/device_dialog.rs:211`), control tooltips
  (`window/controls/tooltips.rs:164` via
  `shared::ui_helpers::spawn_blocking_result`).
- **`std::thread::spawn`.** Cast sync + bridge
  (`overlays_cast/sync.rs:29,71`, `cast_bridge.rs:16,76`), IPTV channel
  parse (`iptv_view/browser.rs:253`), logo fetch
  (`iptv_view/logos.rs:90`). Each pairs with `async_channel::bounded`.
- **`async-channel`.** Sole MPMC primitive; no tokio in graph.
- **mpv render thread.** Owned by `big-mpv-render`; OpenGL render in
  the GTK GL widget on the main thread, callbacks marshalled back via
  glib idle.

## 5. External processes (`Command::new`)

- `src/video/dialogs/format_selection.rs:111` —
  `std::process::Command::new("yt-dlp")`. Argv invariant: `--` before
  URL. INVARIANTS.md §Stage 5 H1 + regression
  `tests/stage05_argv_terminator.rs::ytdlp_argv_uses_double_dash_separator`.
- `src/video/window/init/controls.rs:90` —
  `Command::new("qdbus6")` for KDE keep-above.
- `src/video/window/init/controls.rs:101` —
  `Command::new("wmctrl")` for X11 fallback keep-above.
- `src/video/window/overlays/screenshot.rs:67` —
  `Command::new("ffmpeg")` for video-frame screenshot. Argv invariant:
  `--` before positional output path. INVARIANTS.md §Stage 5 H2 +
  regression
  `tests/stage05_argv_terminator.rs::ffmpeg_screenshot_argv_uses_double_dash_separator`.

No `sh -c` / `bash -c` / `Command::new("sh")` anywhere
(INVARIANTS.md §Stage 5; defended by `no_unguarded_shell_invocations`).

## 6. Security posture

- **Argv terminators.** Every user-controlled positional is preceded by
  `--`. Two grep + integration regression tests
  (`tests/stage05_argv_terminator.rs`).
- **No shell invocation.** Hard rule defended by
  `no_unguarded_shell_invocations` + CI `budgets` grep.
- **Awaited spawns.** All `Command` invocations are `.status()` or
  `.output()`; no orphan children (INVARIANTS.md §Stage 6).
- **`SourceId` discipline.** Five `RefCell<Option<SourceId>>` fields on
  `imp::Window` each with `take().remove()` cancel sites
  (`window/mod.rs:177,211,217,222,233`); INVARIANTS.md §Stage 6.
- **`#[weak]` self captures.** `init/lifecycle.rs:147` pattern: weak
  self + `ControlFlow::Break` on upgrade fail. Auto-cancel on finalize.
- **Atomic writes.** Settings via `big-app-kit` atomic helpers.
- **License-clean.** `deny.toml` with allowance set tracking actual dep
  surface (INVARIANTS.md §Stage 4); one waiver
  (`RUSTSEC-2024-0436` paste via lofty).
- **Typed validators.** M3U import + yt-dlp output parsing reject
  malformed input.
- **No polkit.** Not privileged.
- **`SAFETY` blocks.** Minimal `unsafe` in this crate; FFI surface
  lives in `big-mpv-render` + `big-media-runtime`.

## 7. Testing strategy

- **Unit tests.** Inline per module (controls math, image-adjust
  clamping, IPTV parser).
- **Integration tests.** `tests/stage05_argv_terminator.rs` and other
  pipeline-stage tests under `tests/`.
- **Regression grep tests.** Argv `--` sentinel + no-shell sentinel.
- **Performance budgets.** The CI `budgets` job enforces only the
  stripped release binary size ≤ 22 MiB (current 17.29) and headless
  idle RSS ≤ 256 MiB. Workspace-LOC and cargo-test-runtime figures
  (≤ 18 000 LOC, current 16 579; `cargo test` < 1 s) are tracked
  targets recorded in INVARIANTS.md §Stage 7, not gated in CI.
- **AT-SPI / a11y.** External via `linux-ui-a11y`.
- **VM smoke.** `packaging/pacman/PKGBUILD` build + launch smoke.
- **CI.** `.github/workflows/ci.yml` runs separate jobs: `shellcheck`,
  `fmt`, `clippy` (`-D warnings`), `test` (nextest/cargo test), `deny`,
  `audit`, `machete`, `doc` (`-D warnings`), `complexity` (lizard CCN +
  jscpd), `i18n` (no-op when no `po/`), `budgets`,
  `appstream-validate`, `desktop-validate`, plus label-gated
  `atspi-smoke` (`ci:atspi`) and `vm-smoke` (`ci:vm-smoke`); `semver-checks`
  and `miri` are disabled (`ENABLE_SEMVER`/`ENABLE_MIRI` = false, libs
  only). The cargo invocations do not pass `--locked`.

## 8. Packaging

- **PKGBUILD.** `packaging/pacman/PKGBUILD`.
- **Flatpak.** `packaging/flatpak/` manifest.
- **Nix.** `packaging/nix/` flake.
- **Desktop file.** `resources/applications/br.com.biglinux.VideoPlayer.desktop`.
  (Source app icon: `application-assets/app/br.com.biglinux.VideoPlayer.svg`.)
- **AppStream metainfo.** `resources/metainfo/br.com.biglinux.VideoPlayer.metainfo.xml`.
- **Polkit.** None.
- **i18n.** User-visible strings route through the shared `i18n()` /
  `ni18n()` re-exported from `big-media-runtime`; gettext init via
  `shared::app_base::init_gettext()` (system gettext). Translation
  catalogs (`po/`) are maintained in the `big-media-runtime` repository,
  not in this app repo.
- **Build script.** `build.rs` compiles gresources.
- **Runtime deps.** mpv (via `big-mpv-render`), GStreamer (cast),
  optional yt-dlp, ffmpeg, wmctrl/qdbus6 for window manager control.
