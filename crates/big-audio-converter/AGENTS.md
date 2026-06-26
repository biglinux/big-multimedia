# AGENTS.md — big-audio-converter

GTK4/libadwaita audio transcoder for BigLinux. Thin wrapper: the binary boots
the shared converter UI in `ConverterMode::Audio` and flips
`BIG_MEDIA_AUDIO_CONVERTER=1`. Nearly all behaviour lives in sibling crates.

## What is here

- `src/main.rs` — binary entrypoint; calls `big_audio_converter::main_impl()`.
- `src/lib.rs` — `main_impl()` runs `big_converter_ui::run(RunOptions{ mode: Audio, … })`;
  exports `APP_MODE`, `BOOTSTRAP_ENV_KEY`, `BOOTSTRAP_ENV_VAL`.
- `tests/converter_integration.rs`, `tests/smoke.rs` — link `lib.rs` re-exports +
  `big-media-runtime` (dev-dep) to drive the shared queue / assert argv hardening.
- `resources/`, `packaging/`, `po/` — desktop metadata, packaging sources, gettext catalogs.

That is the whole crate. There is no `src/converter/`, `dev/`, `legacy/`, or
`.agent/` here.

## Where to edit (common tasks)

- Converter window, sidebar, edit/queue pages, dialogs, playback →
  `../big-converter-ui`.
- ffmpeg/ffprobe orchestration, transcode session, scratch dirs, probing →
  `../big-media-runtime`.
- Generic reusable UI/media/task helpers → `big-rust-components` (do not copy
  helpers between apps; promote them to a component crate).
- This repo: only the boot binding, packaging metadata, tests, and docs.

## Build + gate (what actually runs)

```bash
cargo build --release --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features   # or: cargo test --workspace --all-features
cargo deny check
cargo audit -D warnings
cargo machete --with-metadata
for po in po/*.po; do msgfmt -c -v -o /dev/null "$po"; done
```

`Cargo.lock` is committed; the gate uses `--locked`. Full CI matrix:
`.github/workflows/ci.yml`. Budgets enforced: binary ≤ 30 MiB, idle RSS ≤ 150 MiB.

## Invariants and conventions

- Defended properties (argv `--` terminator, hardened `-i`, owned 0700 scratch
  dirs, resource-leak teardown): `INVARIANTS.md` (canonical). Architecture:
  `ARCHITECTURE.md`.
- No `Command::new("sh"|"bash")`, no shell interpolation; path args are `OsStr`,
  guarded with `--` where flags precede them. Enforce in the sibling crates.
- i18n source strings are English; route user-visible text through `i18n()` /
  `ni18n()`. Translations in `po/`.
- Keep the binary thin. Prefer system libraries; record any native-dep exception
  in `docs/native-deps-exceptions.md`.
