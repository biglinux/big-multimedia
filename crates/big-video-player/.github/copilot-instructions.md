# Copilot Instructions

Canonical agent rules: **`AGENTS.md`** at the repo root. Read it first.

Source of truth for contracts and design:

- `ARCHITECTURE.md` — crate layout, async boundaries, every `Command`
  site, security posture.
- `INVARIANTS.md` — enforced invariants with file:line and defenders.
- `SECURITY.md` — threat model, scope, reporting.
- `docs/project-structure.md` — where to edit.

Build / gate: `./dev/quality-check.sh --ci`. `Cargo.lock` is committed;
CI does not pass `--locked`.

Do not duplicate rules here; update `AGENTS.md` or the docs above instead.

> An `.agent/` directory may be generated locally by `bigagents-tools`
> (`./dev/update-bigagents.sh`). It is gitignored, optional, and not part
> of a fresh clone — do not rely on it.
