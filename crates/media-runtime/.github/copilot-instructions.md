# Copilot Instructions

Canonical agent entrypoint: `AGENTS.md` (repo root).

Read in order:

1. `AGENTS.md` — what this is, build/test/lint commands, module map, where to edit.
2. `ARCHITECTURE.md` — module layout, data flow, external processes, security posture.
3. `INVARIANTS.md` — enforced contracts (subprocess argv, paths, DB, i18n, supply chain).
4. `SECURITY.md` — reporting, STRIDE mapping.

Do not duplicate rules here. Update `AGENTS.md` / `ARCHITECTURE.md` / `INVARIANTS.md` instead.

`bigagents-tools` may generate a local `.agent/` map; it is optional, not
committed, and not required to work in this repo.
