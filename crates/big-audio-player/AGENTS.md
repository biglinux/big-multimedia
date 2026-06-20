# AGENTS.md — big-audio-player

GTK4/libadwaita audio player for BigLinux (Rust, edition 2021, GPL-3.0-or-later).
Single binary crate. Playback runtime + i18n come from the sibling
`big-media-runtime` crate; UI helpers from `big-app-kit`,
`big-relm4-components`, `big-media-components`. Keep app code thin; reuse the
shared crates before writing local helpers.

## Start here

- `ARCHITECTURE.md` — module map, async boundaries, security posture (read sec 2 before editing).
- `INVARIANTS.md` — enforced invariants with file:line defenders.
- `docs/project-structure.md` — where to edit.
- `docs/native-deps-exceptions.md` — bundling-exception ledger.

## Build / test / lint (real commands)

```bash
cargo build
make -f Makefile.ci ci      # full gate: fmt clippy test deny audit machete doc complexity i18n budgets
```

Individual gate steps (also runnable directly; see REVIEW.md):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo deny check
cargo audit --ignore RUSTSEC-2024-0436 --deny warnings
cargo machete --with-metadata
```

CI runs each as a separate job in `.github/workflows/ci.yml`.

## Module map (from src/)

- `src/main.rs` — entrypoint; inits logging/gettext/gresources, builds the app. Keep thin.
- `src/resources.rs` — compiled gresource loader.
- `src/audio/app.rs` — `adw::Application`.
- `src/audio/window/` — main `glib::Object` window subclass (not Relm4); state, timers, event sender.
- `src/audio/window/{playback,library,library_scan,browser,detail,radio,radio_search,playlists,download_manager}/` — feature modules.
- `src/audio/converter_launcher.rs` — only `Command::new` site; spawns external converter with `--` argv sentinel.

## Where to edit common tasks

- Playback transitions / events → `src/audio/window/playback/`.
- Library scan / index → `src/audio/library_scan/`, `src/audio/window/library/`.
- Radio / podcasts → `src/audio/window/radio*`, `podcast*.rs`.
- Playlist parse + smart filters → `src/audio/window/playlists*`.
- Shared playback/i18n/db logic → belongs in `big-media-runtime`, not here.

## Conventions

- i18n: user-visible strings go through `big_media_runtime::i18n` (`i18n`/`ni18n`).
  gettext init in `src/main.rs` via `shared::app_base::init_gettext()`.
  No `po/` catalog in this repo — catalogs live in `big-media-runtime`.
- Component reuse: prefer `big-rust-components` / the path-dep crates over local UI helpers (BigLinux Rust policy).
- No vendored native trees; depend on system GTK/mpv/FFmpeg. Record any exception in `docs/native-deps-exceptions.md`.
- Argv safety: never build a converter argv without the `--` end-of-options sentinel (INVARIANTS.md Stage 5 H1).
- Cargo.lock is committed; the gate uses `--locked`.

## Don't

- Don't add a `tokio` runtime — cross-thread signaling is `async-channel` only.
- Don't change `converter_launcher.rs` argv shape without updating INVARIANTS.md.
- Don't copy code between BigLinux apps — promote it to a shared crate.
