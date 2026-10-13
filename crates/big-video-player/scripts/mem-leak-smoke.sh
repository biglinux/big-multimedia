#!/usr/bin/env bash
# mem-leak-smoke.sh: big-video-player memory-leak gate.
#
# Drives each leak-prone open/close target through the shared RSS-slope detector
# named by MEM_LEAK_CHECK: N open/close cycles of a dialog, verdict
# = per-iteration slope of post-close anonymous RSS. Exit 3 from any target =>
# leak. Run it on a VM or a disposable host.
#
# Targets: equalizer, image-adjust.
set -uo pipefail

DETECT="${MEM_LEAK_CHECK:-}"
[[ -x "$DETECT" ]] || { echo "mem-leak-smoke: set MEM_LEAK_CHECK to the RSS-slope detector" >&2; exit 2; }

BIN="${LEAK_BIN:-target/release/big-video-player}"
[[ -x "$BIN" ]] || BIN="target/debug/big-video-player"
[[ -x "$BIN" ]] || { echo "mem-leak-smoke: build the binary first ($BIN)" >&2; exit 2; }

DEST="br.com.biglinux.VideoPlayer"
WPATH="/br/com/biglinux/VideoPlayer/window/1"
ITERS="${LEAK_ITERS:-8}"
THRESH="${LEAK_THRESHOLD_KB:-512}"

# action | window-title (C locale = English msgid).
targets=(
  "show-equalizer|Equalizer"
  "show-image-adjust|Image & Video"
)

out_root="$(mktemp -d "${TMPDIR:-/tmp}/leak-big-video-player.XXXXXX")"
echo "leak reports: $out_root"

rc=0
for t in "${targets[@]}"; do
  action="${t%%|*}"; title="${t#*|}"
  echo ">>> leak target: $action ($title)"
  "$DETECT" --gui --dest "$DEST" --object-path "$WPATH" \
    --action "$action" --window-title "$title" --iters "$ITERS" \
    --threshold-kb "$THRESH" --env LANG=C.UTF-8 --env LC_ALL=C \
    --out "$out_root/$action" \
    -- "$BIN" || rc=$?
done
exit "$rc"
