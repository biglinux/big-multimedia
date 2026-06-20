# Security Policy — big-video-player

## Supported versions

| Version | Supported |
|---------|-----------|
| latest stable (main) | ✅ |
| previous minor       | ✅ (security patches) |
| older                | ❌ |

## Reporting a vulnerability

**Do NOT open public issues for security bugs.**

- **Preferred:** GitHub Security Advisory — https://github.com/biglinux/big-video-player/security/advisories/new
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

- mpv render context lifetime + drop ordering
- OpenGL symbol loader: `dlopen("libGL.so.1", RTLD_LAZY|RTLD_LOCAL)` scoped resolution (NOT `RTLD_DEFAULT`, which is LD_PRELOAD-hijackable)
- IPTV M3U playlist parser bounds (DoS prevention; off-main-thread parse)
- Subtitle (SRT/VTT/ASS) parser bounds
- Path canonicalization for watch-later, playback history, thumbnails
- Atomic settings writes

## Out of scope

- Bugs in upstream mpv / libmpv (report at https://github.com/mpv-player/mpv)
- DRM-protected streams (not supported by design)
- Issues only reproducible with debug builds

## Disclosure

Coordinated. CVE requested when applicable. Credit in CHANGELOG + release notes. 90-day default embargo unless severity dictates faster public.

## Security-relevant invariants

See `INVARIANTS.md` for the enforced contract (FFI lifetimes, dlopen scoping, atomic writes, etc.).

## STRIDE mapping

| Threat | Mitigation |
|--------|-----------|
| Spoofing | no remote control without explicit user gate |
| Tampering | atomic file writes (BigAtomicJsonStore) |
| Repudiation | structured logs via `log` |
| Information disclosure | no telemetry, watch history user-only |
| DoS | parser bounds (M3U, subtitles), thumbnail cache caps |
| Elevation of privilege | no setuid bits, no polkit actions, user-only install |
