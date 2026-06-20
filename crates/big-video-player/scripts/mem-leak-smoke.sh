#!/usr/bin/env bash
# mem-leak-smoke.sh — big-video-player memory-leak gate.
#
# Drives each leak-prone open/close target through the shared RSS-slope detector
# (~/.agents/scripts/mem-leak-check.sh): N open/close cycles of a dialog, verdict
# = per-iteration slope of post-close anonymous RSS. Exit 3 from any target =>
# leak. VM/disposable-host only (the gate wires it under BIGAGENTS_RUN_UI_SMOKE=1).
#
# Targets verified clean 2026-06-19 (slope 0 KiB/open): equalizer, image-adjust.
set -uo pipefail

DETECT="${MEM_LEAK_CHECK:-$HOME/.agents/scripts/mem-leak-check.sh}"
[[ -x "$DETECT" ]] || { echo "mem-leak-smoke: detector missing at $DETECT" >&2; exit 2; }

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

rc=0
for t in "${targets[@]}"; do
  action="${t%%|*}"; title="${t#*|}"
  echo ">>> leak target: $action ($title)"
  "$DETECT" --gui --dest "$DEST" --object-path "$WPATH" \
    --action "$action" --window-title "$title" --iters "$ITERS" \
    --threshold-kb "$THRESH" --env LANG=C.UTF-8 --env LC_ALL=C \
    --out "/tmp/leak-big-video-player-$action" \
    -- "$BIN" || rc=$?
done
exit "$rc"
