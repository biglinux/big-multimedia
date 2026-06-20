# AGENTS.md — big-video-player

Canonical agent rules for this repo. Humans: see README.md.

## What this is

BigLinux **Big Video Player** — thin GTK4/libadwaita video player in Rust
(edition 2021, toolchain 1.93, GPL-3.0-or-later). Single binary, single
crate. Wraps shared crates `big-mpv-render` (libmpv + GL), `big-media-runtime`
(runtime + i18n), and the BigLinux UI kit (`big-app-kit`,
`big-relm4-components`, `big-media-components`). Profile: `media`.

## Build / test / lint (the real commands)

```bash
cargo build                       # debug build
./dev/quality-check.sh --ci       # focused gate (fmt + clippy + tests)
./dev/quality-check.sh --full     # deeper local gate
```

CI mirrors `.github/workflows/ci.yml`: separate jobs `fmt`, `clippy`
(`-D warnings`), `test` (nextest/cargo test), `deny`, `audit`, `machete`,
`doc` (`-D warnings`), `complexity`, `i18n`, `budgets`,
`appstream-validate`, `desktop-validate`, `shellcheck`. Cargo invocations
do **not** pass `--locked`. `Cargo.lock` is committed.

## Crate / module map (from Cargo.toml + src/)

Single package `big-video-player`. Source under `src/`:

- `main.rs` — entrypoint (keep thin); `resources.rs` — gresources loader.
- `video/app.rs` — `adw::Application`; `video/window/` — `imp::Window`
  subclass + init (`init/mpv.rs`, `init/controls.rs`, `init/lifecycle.rs`).
- `video/window/overlays/screenshot.rs` — ffmpeg screenshot path.
- `video/window/overlays_cast/` — Chromecast bridge (mDNS, sync).
- `video/dialogs/format_selection.rs` — yt-dlp probe + format picker.
- `video/iptv_view/` — M3U browser + channel logos.
- `video/sidebar/`, `video/preferences/`, `video/appearance/`.

## Where to edit (common tasks)

- New UI feature → reuse a `big-*` component crate first; app-local code
  goes under `src/video/`. See ARCHITECTURE.md §2 and
  docs/project-structure.md.
- External process call (`Command`) → keep `--` before any user-controlled
  positional; add a regression in `tests/stage05_argv_terminator.rs`.
- User-visible string → wrap with `i18n()` / `ni18n()` (re-exported from
  `big-media-runtime`); catalogs (`po/`) live in `big-media-runtime`.
- Packaging → `resources/`, `packaging/{pacman,nix,flatpak}/`.

## Shared-crate-first policy

Reuse tested crates from `big-media-runtime`, `big-mpv-render`, and
`big-rust-components` before writing app-local helpers, widgets, dialogs,
file pickers, task runners, or media formatting. If a helper is useful in
more than one BigLinux app, move it to the shared crate. Prefer
system/distro libraries; bundling needs an entry in
`docs/native-deps-exceptions.md`.

## Source of truth

- **ARCHITECTURE.md** — purpose, crate layout, state machine, async
  boundaries, every `Command` site, security posture.
- **INVARIANTS.md** — enforced contracts (argv terminators, lifecycle,
  budgets) with file:line and defenders.
- **SECURITY.md** — threat model, scope, reporting.

Do not duplicate those here; point to them.

> An `.agent/` directory may be generated locally by `bigagents-tools`
> (`./dev/update-bigagents.sh`); it is gitignored and not required.
