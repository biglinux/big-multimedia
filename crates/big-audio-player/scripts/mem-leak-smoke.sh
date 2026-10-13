#!/usr/bin/env bash
# Memory-leak gate for big-audio-player.
#
# Drives each leak-prone open/close target through the shared RSS-slope detector
# named by MEM_LEAK_CHECK: N open/close cycles of a dialog, verdict
# = per-iteration slope of post-close anonymous RSS (catches malloc / mmap /
# GObject widget / renderer leaks alike). Exit 3 from any target => leak.
#
# Run it on a VM or a disposable host: the app and its nested software-rendered
# compositor (GSK_RENDERER=cairo, LIBGL_ALWAYS_SOFTWARE=1) can leave processes
# behind.
#
# Targets: preferences, appearance, equalizer. Re-run on any GObject/closure
# change to the dialogs.
set -uo pipefail

DETECT="${MEM_LEAK_CHECK:-}"
[[ -x "$DETECT" ]] || { echo "mem-leak-smoke: set MEM_LEAK_CHECK to the RSS-slope detector" >&2; exit 2; }

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

out_root="$(mktemp -d "${TMPDIR:-/tmp}/leak-big-audio-player.XXXXXX")"
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
