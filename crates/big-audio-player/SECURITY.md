# Security Policy — big-audio-player

## Supported versions

| Version | Supported |
|---------|-----------|
| latest stable (main) | ✅ |
| previous minor       | ✅ (security patches) |
| older                | ❌ |

## Reporting a vulnerability

**Do NOT open public issues for security bugs.**

- **Preferred:** GitHub Security Advisory — https://github.com/biglinux/big-audio-player/security/advisories/new
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

- mpv FFI lifetime + drop ordering (no use-after-free across backends)
- Chromecast HTTP server token gate (`/dev/urandom` 128-bit, path-prefix matching)
- Atomic settings + library DB writes (no torn state on power loss)
- Subprocess argv (lyrics fetch, metadata lookups) — never via shell
- Path canonicalization for media library, settings, cache
- gettext catalog sanitization (translator-supplied markup)

## Out of scope

- Bugs in upstream mpv / libmpv (report at https://github.com/mpv-player/mpv)
- DRM-protected media (not supported by design)
- Issues only reproducible with debug builds

## Disclosure

Coordinated. CVE requested when applicable. Credit in CHANGELOG + release notes. 90-day default embargo unless severity dictates faster public.

## Security-relevant invariants

See `INVARIANTS.md` for the enforced contract (FFI lifetimes, atomic writes, Chromecast token gate, etc.).

## STRIDE mapping

| Threat | Mitigation |
|--------|-----------|
| Spoofing | Chromecast 128-bit session token, no host header echo |
| Tampering | atomic file writes (BigAtomicJsonStore + SQLite WAL) |
| Repudiation | structured logs via `log` + journal |
| Information disclosure | no telemetry, library DB user-only chmod 0600 |
| DoS | parser bounds (M3U, lyrics, cuesheet), thumbnail cache caps |
| Elevation of privilege | no setuid bits, no polkit actions, user-only install |
