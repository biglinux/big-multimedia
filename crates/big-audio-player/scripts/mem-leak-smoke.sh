#!/usr/bin/env bash
# mem-leak-smoke.sh — big-audio-player memory-leak gate.
#
# Drives each leak-prone open/close target through the shared RSS-slope detector
# (~/.agents/scripts/mem-leak-check.sh): N open/close cycles of a dialog, verdict
# = per-iteration slope of post-close anonymous RSS (catches malloc / mmap /
# GObject widget / renderer leaks alike). Exit 3 from any target => leak.
#
# VM/disposable-host only contract (same as ui-action-smoke): the harness runs
# a nested software-render weston (GSK_RENDERER=cairo, LIBGL_ALWAYS_SOFTWARE=1),
# but launching the app + compositor still lingers procs — the quality gate only
# wires this in under BIGAGENTS_RUN_UI_SMOKE=1.
#
# Targets verified clean 2026-06-19 (slope 0 KiB/open): preferences, appearance,
# equalizer. Re-run on any GObject/closure change to the dialogs.
set -uo pipefail

DETECT="${MEM_LEAK_CHECK:-$HOME/.agents/scripts/mem-leak-check.sh}"
[[ -x "$DETECT" ]] || { echo "mem-leak-smoke: detector missing at $DETECT" >&2; exit 2; }

BIN="${LEAK_BIN:-target/release/big-audio-player}"
[[ -x "$BIN" ]] || BIN="target/debug/big-audio-player"
[[ -x "$BIN" ]] || { echo "mem-leak-smoke: build the binary first ($BIN)" >&2; exit 2; }

DEST="br.com.biglinux.AudioPlayer"
WPATH="/br/com/biglinux/AudioPlayer/window/1"
ITERS="${LEAK_ITERS:-8}"
THRESH="${LEAK_THRESHOLD_KB:-512}"

# action | window-title (C locale = English msgid).
targets=(
  "show-preferences|Preferences"
  "show-appearance|Appearance"
  "show-equalizer|Equalizer"
)

rc=0
for t in "${targets[@]}"; do
  action="${t%%|*}"; title="${t#*|}"
  echo ">>> leak target: $action ($title)"
  "$DETECT" --gui --dest "$DEST" --object-path "$WPATH" \
    --action "$action" --window-title "$title" --iters "$ITERS" \
    --threshold-kb "$THRESH" --env LANG=C.UTF-8 --env LC_ALL=C \
    --out "/tmp/leak-big-audio-player-$action" \
    -- "$BIN" || rc=$?
done
exit "$rc"
