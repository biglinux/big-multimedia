# Security Policy — big-audio-converter

## Supported versions

| Version | Supported |
|---------|-----------|
| latest stable (main) | ✅ |
| previous minor       | ✅ (security patches) |
| older                | ❌ |

## Reporting a vulnerability

**Do NOT open public issues for security bugs.**

- **Preferred:** GitHub Security Advisory — https://github.com/biglinux/big-audio-converter/security/advisories/new
- **Backup:** email `security@biglinux.com.br` (PGP key on keys.openpgp.org, fingerprint TBD)

Include: affected version, reproduction steps, impact, suggested fix (optional).

## Response SLA

| Severity | First response | Patch target |
|----------|---------------:|-------------:|
| CRITICAL (RCE, privilege escalation, data loss) | 24h | 72h |
| HIGH (auth bypass, sandbox escape)              | 72h | 7d  |
| MEDIUM (info leak, DoS)                         | 7d  | 30d |
| LOW (defense-in-depth)                          | 14d | next minor |

## In scope

- ffmpeg argv terminator `--` on every transcode invocation (no shell, no glob)
- Input path canonicalization (rejects symlinks pointing outside selected folders)
- Atomic settings + queue state writes (no torn state on crash)
- Output filename sanitization (block traversal, control chars)
- gettext catalog sanitization (translator-supplied markup)
- Subprocess capture bounds (no unbounded stdout buffering)

## Out of scope

- Bugs in upstream ffmpeg (report at https://ffmpeg.org/security.html)
- Codec licensing / DRM (not supported by design)
- Issues only reproducible with debug builds

## Disclosure

Coordinated. CVE requested when applicable. Credit in CHANGELOG + release notes. 90-day default embargo unless severity dictates faster public.

## Security-relevant invariants

See `INVARIANTS.md` for the enforced contract (argv terminator, atomic writes, path canonicalization, etc.).

## STRIDE mapping

| Threat | Mitigation |
|--------|-----------|
| Spoofing | no network surface (offline transcoder) |
| Tampering | atomic file writes for settings + queue state |
| Repudiation | structured logs via `log` |
| Information disclosure | output goes to user-selected dirs; no telemetry |
| DoS | per-job timeout, capture-output size cap, queue length cap |
| Elevation of privilege | no setuid bits, no polkit actions, user-only install |
