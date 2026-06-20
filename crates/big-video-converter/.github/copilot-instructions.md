
# Copilot Instructions

Canonical orientation:

- `ARCHITECTURE.md` — design, crate layout, and cross-repo map.
- `INVARIANTS.md` — enforced contract (argv terminator, dlopen scoping,
  resource lifecycle, perf/a11y budgets).
- `README.md` — what this is, build/test commands.
- `docs/project-structure.md` — where to edit.

This is a thin wrapper. The converter UI (window, sidebar, dialogs, edit/queue
pages, settings) lives upstream in
`../big-rust-components/crates/big-converter-ui`; the multimedia backend lives
in `../big-media-runtime`. Do not recreate that logic in this repo.

Build/test: `cargo build`, then `cargo nextest run --workspace --all-features`.
Full gate matrix: `REVIEW.md`.

Do not duplicate rules here. Update `ARCHITECTURE.md` / `INVARIANTS.md` instead.
