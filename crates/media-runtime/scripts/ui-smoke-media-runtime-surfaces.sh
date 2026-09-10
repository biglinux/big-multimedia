#!/usr/bin/env bash
# KWin/AT-SPI smoke for big-media-runtime owned GTK surfaces.
set -euo pipefail

SCRIPT_NAME="${0##*/}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
EVIDENCE_DIR="${OUT_DIR:-${BIGLINUX_UI_EVIDENCE_DIR:-$ROOT_DIR/target/ui-smoke/media-runtime-surfaces}}"
BUILD_PROFILE="${UI_BUILD_PROFILE:-debug}"

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
*) fail "unsupported UI_BUILD_PROFILE: $BUILD_PROFILE" ;;
esac

[[ -x "$HEADLESS_RUNNER" && -f "$ATSPI_TOOL" ]] ||
	fail "KWin/AT-SPI runner missing under $SKILL_SCRIPT_DIR"

mkdir -p "$EVIDENCE_DIR"
printf 'cargo build --manifest-path %q --locked --features ui --example ui_surface_probe %s\n' \
	"$ROOT_DIR/Cargo.toml" "${build_arguments[*]:-}" >"$EVIDENCE_DIR/command.txt"

cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --locked --features ui --example ui_surface_probe "${build_arguments[@]}"

run_surface() {
	local surface_name="$1"
	local wait_query="$2"
	local automation_body="$3"
	local surface_dir="$EVIDENCE_DIR/$surface_name"
	local automation_file="$surface_dir/automation.sh"

	mkdir -p "$surface_dir"
	{
		printf '#!/usr/bin/env bash\n'
		printf 'set -euo pipefail\n'
		printf 'ATSPI=%q\n' "$ATSPI_TOOL"
		printf '%s\n' "$automation_body"
		# shellcheck disable=SC2016
		printf 'python3 "$ATSPI" layout --max-depth 30 >%q || true\n' "$surface_dir/final.layout.txt"
		# shellcheck disable=SC2016
		printf 'python3 "$ATSPI" targets --max-depth 30 >%q || true\n' "$surface_dir/final.targets.txt"
		# shellcheck disable=SC2016
		printf 'python3 "$ATSPI" dump --max-depth 30 >%q || true\n' "$surface_dir/final.tree.txt"
	} >"$automation_file"
	chmod 700 "$automation_file"

	env \
		BIGLINUX_UI_SESSION_MODE=headless \
		BIGLINUX_VISIBLE_HOST_DISPLAY_FORBIDDEN=1 \
		BIG_MEDIA_RUNTIME_PROBE_SURFACE="$surface_name" \
		XDG_CONFIG_HOME="$surface_dir/config" \
		XDG_CACHE_HOME="$surface_dir/cache" \
		XDG_DATA_HOME="$surface_dir/data" \
		AUDIT_APP=ui_surface_probe \
		"$HEADLESS_RUNNER" \
		--compositor kwin \
		--log-dir "$surface_dir/session" \
		--screenshot "$surface_dir/$surface_name.png" \
		--wait-ready-query "=$wait_query" \
		--inspect-atspi \
		--audit \
		--automation-file "$automation_file" \
		--automation-delay 1 \
		--dump-before "$surface_dir/before.tree.txt" \
		--dump-after "$surface_dir/after.tree.txt" \
		--dump-diff "$surface_dir/tree.diff" \
		-- "$binary_path"

	file "$surface_dir/$surface_name.png" | grep -q '1280 x 720' ||
		fail "expected 1280x720 screenshot for $surface_name"
	[[ -s "$surface_dir/$surface_name.grid.png" ]] ||
		fail "missing grid screenshot for $surface_name"
}

# shellcheck disable=SC2016
run_surface audio-editor "Audio editor" '
python3 "$ATSPI" wait-for --query "=Audio editor" --role window --timeout-ms 20000 >/dev/null
python3 "$ATSPI" wait-for --query "=Equalizer" --role button --timeout-ms 10000 >/dev/null
python3 "$ATSPI" wait-for --query "=Convert this file" --role button --timeout-ms 10000 >/dev/null
'

# shellcheck disable=SC2016
run_surface video-editor "Video editor" '
python3 "$ATSPI" wait-for --query "=Video editor" --role window --timeout-ms 20000 >/dev/null
python3 "$ATSPI" wait-for --query "=Convert this file" --role button --timeout-ms 10000 >/dev/null
python3 "$ATSPI" wait-for --query "=Back to queue" --role button --timeout-ms 10000 >/dev/null
'

# shellcheck disable=SC2016
run_surface equalizer "Equalizer" '
python3 "$ATSPI" wait-for --query "=Equalizer" --role window --timeout-ms 20000 >/dev/null
python3 "$ATSPI" wait-for --query "=Reset to Flat" --role button --timeout-ms 10000 >/dev/null
python3 "$ATSPI" wait-for --query "=Equalizer band 1 kHz" --timeout-ms 10000 >/dev/null
'

# shellcheck disable=SC2016
run_surface noise-reduction "Noise reduction settings" '
python3 "$ATSPI" wait-for --query "=Noise reduction settings" --role window --timeout-ms 20000 >/dev/null
python3 "$ATSPI" wait-for --query "=Noise reduction model" --timeout-ms 10000 >/dev/null
python3 "$ATSPI" wait-for --query "=Voice recovery" --timeout-ms 10000 >/dev/null
'

printf 'media-runtime-surfaces\tOK\t%s\n' "$EVIDENCE_DIR" >"$EVIDENCE_DIR/summary.tsv"
printf '%s complete. Artifacts: %s\n' "$SCRIPT_NAME" "$EVIDENCE_DIR" >&2
