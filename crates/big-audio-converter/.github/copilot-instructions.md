# Copilot Instructions

Canonical project context: `AGENTS.md` (root).

Then read, in order:

- `ARCHITECTURE.md` — purpose, crate layout, async boundaries, external processes.
- `INVARIANTS.md` — defended security/resource properties.
- `docs/project-structure.md` — where to edit.

This crate is a thin wrapper: `src/` holds only `main.rs` + `lib.rs`. The
converter UI/state lives in `../big-rust-components/crates/big-converter-ui`
and ffmpeg orchestration in `../big-media-runtime`. Most edits belong in those
sibling crates, not here.

Do not duplicate rules in this file. Update `AGENTS.md` or the docs above
instead.
