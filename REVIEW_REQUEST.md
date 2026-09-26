# Code Review Request — big-multimedia

## What this project is

`big-multimedia` is an open-source **desktop media player** application for the
BigLinux distribution, written in Rust with GTK4/Relm4. It ships two graphical
programs for end users:

- **big-video-player** — plays video files, IPTV and online video, and can cast
  to a Chromecast.
- **big-audio-player** — plays a music library, playlists, radio and podcasts.

Playback uses the `mpv` library. Neither player converts media: marked cuts
are handed to separate converter GUIs. Licensed GPL-3.0-or-later. Full source
is in the attached archive.

This is ordinary consumer desktop software — the equivalent of VLC or
Rhythmbox. Network use is limited to media streams, metadata lookups (through
the system `curl`) and a token-gated local HTTP server for casting.

## What I am asking for

A normal **software-engineering code review** aimed at improving code quality.
Please read the source and documentation and report:

- Bugs and logic errors.
- Non-idiomatic Rust; places to simplify or reduce duplication.
- Architecture and API design feedback.
- Performance opportunities (startup time, memory, UI responsiveness).
- Error-handling gaps and resource leaks.
- Documentation clarity.

Concrete suggested diffs or before/after snippets are welcome.

## Working constraints

You do **not** need to build or run anything. The review environment has no
Linux desktop, no Rust toolchain, and no `mpv`/`ffmpeg` installed — that is
expected. Please reason from the source code, the `Cargo.toml`/`Cargo.lock`
dependency set, and the per-crate `README.md` files (plus
`crates/media-runtime/ARCHITECTURE.md`). Library
documentation you already have access to (GTK4, Relm4, mpv, ffmpeg) is enough to
evaluate the code.

## Where to start

- `README.md` — workspace overview and crate map.
- `crates/big-video-player/` and `crates/big-audio-player/` — the two user apps.
- `crates/foundation/` and `crates/ui/` — shared component crates.
- `Cargo.toml` at the root — workspace members and shared dependencies.
