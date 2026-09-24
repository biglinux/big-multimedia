# big-media-runtime

Shared GPL runtime for BigLinux multimedia apps.

This repository holds code that is shared by:

- `big-video-player`
- `big-audio-player`
- `big-video-converter`
- `big-audio-converter`

Generic components stay in
[big-multimedia](https://github.com/biglinux/big-multimedia). This
runtime may depend on those crates, but it should not live inside that repo
because it contains product-specific multimedia application code.

## Boundary

- Keep mpv backend, converter queues/profiles/FFmpeg command generation,
  Chromecast, music library, settings store, and multimedia-specific backend
  workflows here.
- Keep app launch, windows, dialogs, sidebars, and converter UI flows in the
  owning app repositories.
- Move reusable UI widgets, file pickers, task runners, rows, dialogs, media
  formatting helpers, and audio-effect contracts to `big-multimedia` when
  they are useful outside these four apps.
- Do not copy shared runtime code into app repositories.

## Build

```bash
cargo build --locked
```

Quality gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features    # or: cargo test --all-features --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo deny check
cargo audit -D warnings
cargo machete --with-metadata
```

`cargo semver-checks --workspace` and `cargo miri test --lib` also run in CI.
Run the full block before release or after dependency, security,
native-library, or public API changes. See `AGENTS.md` for the agent-facing
map and `REVIEW.md` for the reproduce commands.
