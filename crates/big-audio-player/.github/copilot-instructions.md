# Copilot Instructions

Canonical project context (all committed in this repo):

- `AGENTS.md` — entrypoint: build/test/lint commands, module map, conventions.
- `ARCHITECTURE.md` — crate map + async boundaries + security posture.
- `INVARIANTS.md` — enforced invariants with file:line defenders.
- `docs/project-structure.md` — where to edit.

Before editing, read `ARCHITECTURE.md` sec 2 (module map) and the relevant
`INVARIANTS.md` stage. Run the gate via `make -f Makefile.ci ci`, or the
individual cargo commands in `REVIEW.md` / `AGENTS.md`.

A `.agent/` directory (project map, skill router, active skills) may be
generated locally by bigagents-tools. It is optional, not required, and not
committed — do not rely on it being present.

Do not duplicate rules here. Update `AGENTS.md` instead.
