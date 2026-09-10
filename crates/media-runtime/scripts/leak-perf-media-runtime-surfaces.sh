#!/usr/bin/env bash
# Leak/perf probe for big-media-runtime owned GTK dialogs.
set -euo pipefail

SCRIPT_NAME="${0##*/}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
EVIDENCE_DIR="${OUT_DIR:-${BIGLINUX_UI_EVIDENCE_DIR:-$ROOT_DIR/target/leak-perf/media-runtime-surfaces}}"
BUILD_PROFILE="${LEAK_BUILD_PROFILE:-debug}"

SKILL_SCRIPT_DIR="${ATSPI_SKILL_DIR:-$HOME/.agents/skills/linux-ui-a11y/scripts}"
HEADLESS_RUNNER="$SKILL_SCRIPT_DIR/run_headless_wayland_atspi.sh"
ATSPI_TOOL="$SKILL_SCRIPT_DIR/atspi_tool.py"

fail() {
	printf '%s: %s\n' "$SCRIPT_NAME" "$*" >&2
	exit 1
}

case "$BUILD_PROFILE" in
release)
	build_arguments=(--release)
	binary_path="$ROOT_DIR/target/release/examples/ui_surface_probe"
	;;
debug)
	build_arguments=()
	binary_path="$ROOT_DIR/target/debug/examples/ui_surface_probe"
	;;
*) fail "unsupported LEAK_BUILD_PROFILE: $BUILD_PROFILE" ;;
esac

[[ -x "$HEADLESS_RUNNER" && -f "$ATSPI_TOOL" ]] ||
	fail "KWin/AT-SPI runner missing under $SKILL_SCRIPT_DIR"

ITERATIONS="${LEAK_ITERS:-16}"
THRESHOLD_KIB="${LEAK_THRESHOLD_KIB:-768}"
SESSION_DIR="$EVIDENCE_DIR/dialog-cycles"
AUTOMATION_FILE="$SESSION_DIR/automation.sh"

mkdir -p "$SESSION_DIR"
printf 'cargo build --manifest-path %q --locked --features ui --example ui_surface_probe %s\n' \
	"$ROOT_DIR/Cargo.toml" "${build_arguments[*]:-}" >"$EVIDENCE_DIR/command.txt"

cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --locked --features ui --example ui_surface_probe "${build_arguments[@]}"

cat >"$AUTOMATION_FILE" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

ATSPI="${ATSPI_TOOL:?}"
SESSION_DIR="${SESSION_DIR:?}"
PROBE_RESULTS="$SESSION_DIR/probe-results.json"

for _ in $(seq 1 120); do
	if [ -s "$PROBE_RESULTS" ]; then
		break
	fi
	sleep 0.25
done

[ -s "$PROBE_RESULTS" ] || {
	echo "timed out waiting for $PROBE_RESULTS" >&2
	exit 1
}

python3 "$ATSPI" layout --max-depth 30 >"$SESSION_DIR/final.layout.txt" || true
python3 "$ATSPI" targets --max-depth 30 >"$SESSION_DIR/final.targets.txt" || true
python3 "$ATSPI" dump --max-depth 30 >"$SESSION_DIR/final.tree.txt" || true
EOF
chmod 700 "$AUTOMATION_FILE"

env \
	ATSPI_TOOL="$ATSPI_TOOL" \
	SESSION_DIR="$SESSION_DIR" \
	BIGLINUX_UI_SESSION_MODE=headless \
	BIGLINUX_VISIBLE_HOST_DISPLAY_FORBIDDEN=1 \
	BIG_MEDIA_RUNTIME_PROBE_SURFACE=leak-cycle \
	BIG_MEDIA_RUNTIME_PROBE_DIR="$SESSION_DIR" \
	BIG_MEDIA_RUNTIME_LEAK_ITERS="$ITERATIONS" \
	BIG_MEDIA_RUNTIME_LEAK_THRESHOLD_KIB="$THRESHOLD_KIB" \
	XDG_CONFIG_HOME="$SESSION_DIR/config" \
	XDG_CACHE_HOME="$SESSION_DIR/cache" \
	XDG_DATA_HOME="$SESSION_DIR/data" \
	AUDIT_APP=ui_surface_probe \
	"$HEADLESS_RUNNER" \
	--compositor kwin \
	--log-dir "$SESSION_DIR/session" \
	--wait-ready-query "=Media runtime leak probe" \
	--inspect-atspi \
	--audit \
	--diagnostic-no-screenshot \
	--automation-file "$AUTOMATION_FILE" \
	--automation-delay 0.5 \
	--dump-before "$SESSION_DIR/before.tree.txt" \
	--dump-after "$SESSION_DIR/after.tree.txt" \
	--dump-diff "$SESSION_DIR/tree.diff" \
	-- "$binary_path"

[[ -s "$SESSION_DIR/probe-result.txt" ]] || fail "missing probe-result.txt"
[[ -s "$SESSION_DIR/probe-results.json" ]] || fail "missing probe-results.json"
grep -q '"status": "clean"' "$SESSION_DIR/probe-results.json" ||
	fail "leak/perf probe did not report clean status"

python3 "$ATSPI_TOOL" audit --app ui_surface_probe >"$SESSION_DIR/final-audit.txt" 2>/dev/null || true
printf 'media-runtime-dialog-cycles\tOK\t%s\n' "$EVIDENCE_DIR" >"$EVIDENCE_DIR/summary.tsv"
printf '%s complete. Artifacts: %s\n' "$SCRIPT_NAME" "$EVIDENCE_DIR" >&2
