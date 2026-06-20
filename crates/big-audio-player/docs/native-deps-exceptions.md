# Native Dependency Exceptions

## rustls crypto providers: aws-lc-sys, ring

- Owner: BigLinux multimedia maintainers.
- Reason: shared `big-media-runtime` pulls `rust_cast` and `ureq`, which pull
  `rustls` and compile `aws-lc-sys` and `ring` for TLS/crypto support. No
  suitable system-library backend is exposed by these dependency paths today.
- Pacman check: `pacman -Ss '^aws-lc$|^ring$|aws-lc|libring|rust-ring'`
  returned no matching system package on 2026-05-13.
- License: `ring` declares `Apache-2.0 AND ISC`; `aws-lc-sys` declares
  permissive Apache-2.0/ISC/MIT/BSD-style terms in the crate manifest.
- Update plan: keep `cargo audit`, `trivy fs .`, and lockfile updates in the
  release gate. Review this exception when `rust_cast`, `ureq`, or `rustls`
  offers a system-backed crypto/TLS path suitable for BigLinux packaging.
- Removal trigger: remove this exception and switch to system package/dynamic
  library when a maintained pacman package or upstream system-backed backend
  becomes available without losing required Cast/HTTP functionality.
