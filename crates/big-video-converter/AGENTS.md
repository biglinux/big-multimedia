# AGENTS.md — big-video-converter

GPU-aware GTK4/libadwaita video converter for BigLinux. Thin wrapper: the
binary calls `big_converter_ui::run(RunOptions { mode: Video, … })`.

## What lives here

- `src/main.rs` — binary entrypoint; calls `big_video_converter::main_impl`.
- `src/lib.rs` — delegates to `big_converter_ui::run(...)`; exports `APP_MODE =
  ConverterMode::Video`.
- `tests/converter_integration.rs`, `tests/smoke.rs` — drive the shared queue
  against fixtures; assert ffmpeg argv shape (never spawned).
- `resources/`, `packaging/`, `po/` — desktop metadata, packages, translations.

There is NO app-owned converter source in this repo. The converter window,
sidebar, dialogs, edit/queue pages, render loop, and settings live upstream in
`../big-converter-ui`. The multimedia backend
(ffmpeg/ffprobe orchestration, GPU detect, cast/transcode) lives in
`../big-media-runtime`.

## Build / test / lint (real commands)

```sh
cargo build
cargo nextest run --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Full release gate (fmt, clippy, doc, tests, deny, audit, machete) and exact
flags: `REVIEW.md`. CI: `.github/workflows/ci.yml` (note: CI does not pass
`--locked` yet; `Cargo.lock` is committed).

## Where to edit for common tasks

- Boot mode / log filter / run options → `src/lib.rs` (`main_impl`).
- Converter UI / settings / edit page / queue → upstream
  `big-converter-ui` (NOT here).
- ffmpeg/GPU/cast backend → upstream `big-media-runtime` (NOT here).
- Desktop entry / AppStream / icon → `resources/`.
- Packaging → `packaging/{pacman,nix,flatpak}/`.
- Translations → `po/` (see `po/LINGUAS`, `po/POTFILES.in`).

## Invariants & conventions

- Enforced contract: `INVARIANTS.md`. Architecture + cross-repo map:
  `ARCHITECTURE.md`. Do not duplicate them here.
- Reuse tested crates from `big-rust-components` / `big-media-runtime`; do not
  recreate shared UI or runtime locally.
- Do not move app-owned UI back into the runtime crate.
- i18n source strings are English; translations live in `po/`.
- Prefer distro/system native libs (GTK, libadwaita, mpv, ffmpeg). Bundling
  needs an exception recorded in `docs/native-deps-exceptions.md`.
- No shell interpolation in subprocess calls; argv vectors with `--` separator.
