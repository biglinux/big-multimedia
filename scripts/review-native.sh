#!/usr/bin/env bash
# Native gate. Missing tooling is BLOCKED (77), never success. No GUI/GPU certification.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$ROOT/review-native-results}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
cd "$ROOT"
SUMMARY="$OUT/results.tsv"
printf 'check\tstatus\texit_code\tlog\n' > "$SUMMARY"
blocked=0
for tool in cargo rustc rustfmt pkg-config msgfmt xgettext python3; do
    if ! command -v "$tool" > /dev/null 2>&1; then
        printf '%s\tBLOCKED\t77\tmissing tool\n' "$tool" >> "$SUMMARY"
        blocked=1
    fi
done
if [[ $blocked == 0 ]]; then
    for dependency in 'gtk4 >= 4.22' 'libadwaita-1 >= 1.9' 'mpv' 'sqlite3'; do
        if ! pkg-config --exists "$dependency"; then
            printf '%s\tBLOCKED\t77\tmissing native development dependency\n' "$dependency" >> "$SUMMARY"
            blocked=1
        fi
    done
fi
if [[ $blocked != 0 ]]; then
    cat "$SUMMARY"
    printf '\nBLOCKED: no Rust build/test or GUI/GPU validation has passed.\n' >&2
    exit 77
fi
{
    rustc --version --verbose
    cargo --version
    pkg-config --modversion gtk4 libadwaita-1 mpv sqlite3
    printf 'DISPLAY=%s\nWAYLAND_DISPLAY=%s\n' "${DISPLAY:-}" "${WAYLAND_DISPLAY:-}"
} > "$OUT/environment.txt" 2>&1
failed=0
run() {
    local name="$1"; shift
    "$@" > "$OUT/$name.log" 2>&1
    local code=$?
    local status=PASS
    if [[ $code != 0 ]]; then status=FAIL; failed=1; fi
    printf '%s\t%s\t%s\t%s\n' "$name" "$status" "$code" "$name.log" >> "$SUMMARY"
}
run static python3 scripts/review-check.py --json "$OUT/static.json"
run format cargo fmt --all -- --check
run gettext msgfmt --check --check-format -o "$OUT/pt_BR.mo" crates/media-runtime/po/pt_BR.po
run check-players cargo check --locked -p big-audio-player -p big-video-player --all-targets
run test-os cargo test --locked -p big-os-kit --all-features
run test-data cargo test --locked -p big-media-data --all-features
run test-policy cargo test --locked -p big-mpv-render
run test-runtime cargo test --locked -p big-media-runtime --features ui --lib -- --test-threads=1
run test-players cargo test --locked -p big-audio-player -p big-video-player -- --test-threads=1
run check-optional cargo check --locked -p big-audio-player -p big-video-player --all-features --all-targets
run clippy cargo clippy --locked -p big-audio-player -p big-video-player --all-targets -- -D warnings
cat "$SUMMARY"
printf '\nNative build/unit results only; complete the separate AT-SPI, playback, GPU and performance matrix.\n'
exit "$failed"
