#!/usr/bin/env bash
# BigMultimedia dialog leak/perf gate.
#
# Builds the audio/video player binaries and drives stable menu dialogs through
# AT-SPI actions in a private KWin session. Each dialog run records Anonymous
# and Pss after every close; a steady Anonymous slope above the threshold fails.
#
# ATSPI_TOOLS_DIR names the directory holding run_headless_wayland_atspi.sh
# (private KWin + AT-SPI session runner) and atspi_tool.py.
set -euo pipefail

SCRIPT_NAME="${0##*/}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
EVIDENCE_DIR="${OUT_DIR:-${BIGLINUX_UI_EVIDENCE_DIR:-$ROOT_DIR/target/leak-perf/dialogs}}"
BUILD_PROFILE="${LEAK_BUILD_PROFILE:-release}"

TOOLS_DIR="${ATSPI_TOOLS_DIR:-}"
HEADLESS_RUNNER="$TOOLS_DIR/run_headless_wayland_atspi.sh"
ATSPI_TOOL="$TOOLS_DIR/atspi_tool.py"

usage() {
	cat <<EOF
Usage: $SCRIPT_NAME [--out DIR] [--profile release|debug]

Builds BigMultimedia audio/video players and measures dialog open/close anonymous-memory
growth in a private KWin + AT-SPI session. Needs ATSPI_TOOLS_DIR.
EOF
}

fail() {
	printf '%s: %s\n' "$SCRIPT_NAME" "$*" >&2
	exit 1
}

while [ "$#" -gt 0 ]; do
	case "$1" in
	--out)
		[ "$#" -ge 2 ] || fail "missing value after --out"
		EVIDENCE_DIR="$2"
		shift 2
		;;
	--profile)
		[ "$#" -ge 2 ] || fail "missing value after --profile"
		BUILD_PROFILE="$2"
		shift 2
		;;
	-h | --help)
		usage
		exit 0
		;;
	*) fail "unknown argument: $1" ;;
	esac
done

[[ -n "$TOOLS_DIR" && -x "$HEADLESS_RUNNER" && -f "$ATSPI_TOOL" ]] || {
	echo "$SCRIPT_NAME: set ATSPI_TOOLS_DIR to the directory with run_headless_wayland_atspi.sh and atspi_tool.py" >&2
	exit 2
}

case "$BUILD_PROFILE" in
release)
	build_arguments=(--release)
	binary_directory="$ROOT_DIR/target/release"
	;;
debug)
	build_arguments=()
	binary_directory="$ROOT_DIR/target/debug"
	;;
*) fail "unsupported profile: $BUILD_PROFILE" ;;
esac

mkdir -p "$EVIDENCE_DIR"
COMMAND_FILE="$EVIDENCE_DIR/command.txt"
SUMMARY_TSV="$EVIDENCE_DIR/summary.tsv"
: >"$SUMMARY_TSV"

{
	printf 'cargo build --manifest-path %q --locked --bin big-audio-player --bin big-video-player' "$ROOT_DIR/Cargo.toml"
	if [ "${#build_arguments[@]}" -gt 0 ]; then
		printf ' %q' "${build_arguments[@]}"
	fi
	printf '\n'
	printf 'OUT_DIR=%q %q\n' "$EVIDENCE_DIR" "$SCRIPT_DIR/leak-perf-dialogs.sh"
} >"$COMMAND_FILE"

cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --locked --bin big-audio-player --bin big-video-player "${build_arguments[@]}"

ITERATIONS="${LEAK_ITERS:-16}"
THRESHOLD_KIB="${LEAK_THRESHOLD_KB:-512}"
CLOSE_SLEEP="${LEAK_CLOSE_SLEEP:-0.4}"

# key | app name | binary | ready title | AT-SPI action | title | accessible role.
dialog_specs=(
	"audio_preferences|audio|$binary_directory/big-audio-player|Big Audio Player|win.show-preferences|Preferences|dialog"
	"audio_appearance|audio|$binary_directory/big-audio-player|Big Audio Player|win.show-appearance|Appearance|window"
	"audio_equalizer|audio|$binary_directory/big-audio-player|Big Audio Player|win.show-equalizer|Equalizer|window"
	"video_preferences|video|$binary_directory/big-video-player|Big Video Player|win.preferences|Preferences|dialog"
	"video_appearance|video|$binary_directory/big-video-player|Big Video Player|win.appearance|Appearance|window"
)

write_automation_script() {
	local automation_path="$1"
	cat >"$automation_path" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

ATSPI="${ATSPI_TOOL:?}"
ACTION_NAME="${ACTION_NAME:?}"
DIALOG_TITLE="${DIALOG_TITLE:?}"
DIALOG_ROLE="${DIALOG_ROLE:?}"
READY_TITLE="${READY_TITLE:?}"
ITERATIONS="${ITERATIONS:?}"
SAMPLES_CSV="${SAMPLES_CSV:?}"
CLOSE_SLEEP="${CLOSE_SLEEP:?}"
BINARY_PATH="${BINARY_PATH:?}"

application_pid="$(cat "${APPLICATION_PID_FILE:?}")"
[[ "$application_pid" =~ ^[0-9]+$ && "$(readlink -f "/proc/$application_pid/exe")" == "$(readlink -f "$BINARY_PATH")" ]] || {
	echo "recorded application PID does not match $BINARY_PATH" >&2
	exit 1
}

sample_memory() {
	awk -v phase="$1" -v timestamp="$(date +%s%3N)" '
		$1 == "Anonymous:" { anonymous = $2 }
		$1 == "Pss:" { pss = $2 }
		END {
			if (anonymous == "" || pss == "") exit 1;
			printf "%s,%s,%s,%s\n", phase, timestamp, anonymous, pss;
		}
	' "/proc/$application_pid/smaps_rollup" >>"$SAMPLES_CSV"
}

activate_atspi_action() {
	python3 - "$ATSPI" "$ACTION_NAME" <<'PY'
import ast
import re
import subprocess
import sys

atspi_tool = sys.argv[1]
action_name = sys.argv[2]
dump_lines = subprocess.run(
    [
        sys.executable,
        atspi_tool,
        "dump",
        "--require-action",
        "--max-depth",
        "12",
    ],
    check=True,
    text=True,
    capture_output=True,
).stdout.splitlines()

current_reference = None
for line in dump_lines:
    match = re.search(r"dest=([^ ]+) path=(\S+)", line)
    if match:
        current_reference = (match.group(1), match.group(2))
        continue

    stripped_line = line.strip()
    if not stripped_line.startswith("actions=") or current_reference is None:
        continue

    actions = ast.literal_eval(stripped_line.split("=", 1)[1])
    if action_name not in actions:
        continue

    action_index = actions.index(action_name)
    result = subprocess.run(
        [
            sys.executable,
            atspi_tool,
            "do-action",
            current_reference[0],
            current_reference[1],
            str(action_index),
        ],
        text=True,
    )
    raise SystemExit(result.returncode)

print(f"AT-SPI action not found: {action_name}", file=sys.stderr)
raise SystemExit(20)
PY
}

: >"$SAMPLES_CSV"
python3 "$ATSPI" wait-for --query "=$READY_TITLE" --role window --timeout-ms 20000 --pid "$application_pid" >/dev/null
sample_memory start

for ((iteration = 1; iteration <= ITERATIONS; iteration++)); do
	activate_atspi_action
	python3 "$ATSPI" wait-for --query "=$DIALOG_TITLE" --role "$DIALOG_ROLE" --timeout-ms 10000 --pid "$application_pid" >/dev/null
	sample_memory open
	if [[ "$DIALOG_ROLE" == dialog ]]; then
	python3 "$ATSPI" wait-act-scoped --scope-descendant-query "=$DIALOG_TITLE" --scope-descendant-role dialog --include-scope-self --target-query "=Close" --target-role button --timeout-ms 10000 >/dev/null
	else
		python3 "$ATSPI" wait-act-containing --descendant-query "=$DIALOG_TITLE" --descendant-role window --ancestor-role window --include-self --action-name window.close --timeout-ms 10000 >/dev/null
	fi
	python3 "$ATSPI" wait-gone --query "=$DIALOG_TITLE" --role "$DIALOG_ROLE" --timeout-ms 10000 --pid "$application_pid" >/dev/null
	sleep "$CLOSE_SLEEP"
	sample_memory closed
done
EOF
	chmod 700 "$automation_path"
}

write_verdict() {
	local samples_path="$1"
	local verdict_path="$2"
	awk -F, -v threshold="$THRESHOLD_KIB" '
		$1=="closed" {
			n++;
			x[n]=n;
			y[n]=$3;
			if (n == 1) first=$3;
			last=$3;
		}
		END {
			if (n < 3) {
				print "INCONCLUSIVE: need >=3 close samples (got " n ")";
				exit 2;
			}
			sx=sy=sxx=sxy=0;
			for (i=1; i<=n; i++) {
				sx+=x[i];
				sy+=y[i];
				sxx+=x[i]*x[i];
				sxy+=x[i]*y[i];
			}
			slope=(n*sxy - sx*sy)/(n*sxx - sx*sx);
			half=int(n/2);
			first_average=0;
			last_average=0;
			for (i=2; i<=half; i++) first_average+=y[i]-y[i-1];
			if (half > 1) first_average/=(half-1);
			for (i=half+1; i<=n; i++) last_average+=y[i]-y[i-1];
			if (n-half > 0) last_average/=(n-half);
			printf "Anonymous after close: first=%dKiB last=%dKiB over %d iters\n", first, last, n;
			printf "per-iteration slope = %.0f KiB/open (early avg %.0f, late avg %.0f KiB/open)\n", slope, first_average, last_average;
			if (slope <= threshold) {
				printf "VERDICT: clean (slope %.0f <= %d KiB/open)\n", slope, threshold;
				exit 0;
			}
			if (last_average < first_average*0.4) {
				printf "VERDICT: GROWS but DECELERATING (%.0f->%.0f) - likely cache warming\n", first_average, last_average;
				exit 0;
			}
			printf "VERDICT: GROWTH %.0f KiB/open (steady)\n", slope;
			exit 3;
		}
	' "$samples_path" >"$verdict_path"
}

overall_status=0
for dialog_spec in "${dialog_specs[@]}"; do
	IFS='|' read -r dialog_key application_name binary_path ready_title atspi_action dialog_title dialog_role <<<"$dialog_spec"
	[[ -x "$binary_path" ]] || fail "built binary missing: $binary_path"

	dialog_artifact_dir="$EVIDENCE_DIR/$dialog_key"
	automation_script="$dialog_artifact_dir/automation.sh"
	samples_path="$dialog_artifact_dir/samples.csv"
	verdict_path="$dialog_artifact_dir/probe-result.txt"
	mkdir -p "$dialog_artifact_dir/session" \
		"$dialog_artifact_dir/xdg-config" \
		"$dialog_artifact_dir/xdg-cache" \
		"$dialog_artifact_dir/xdg-data" "$dialog_artifact_dir/home"
	write_automation_script "$automation_script"
	cat >"$dialog_artifact_dir/launch.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$$" >"${APPLICATION_PID_FILE:?}"
exec "$@"
EOF

	echo ">>> leak target: $dialog_key ($dialog_title via $atspi_action)"
	set +e
	ATSPI_TOOL="$ATSPI_TOOL" \
		ACTION_NAME="$atspi_action" \
		DIALOG_TITLE="$dialog_title" \
		DIALOG_ROLE="$dialog_role" \
		READY_TITLE="$ready_title" \
		ITERATIONS="$ITERATIONS" \
		SAMPLES_CSV="$samples_path" \
		CLOSE_SLEEP="$CLOSE_SLEEP" \
		BINARY_PATH="$binary_path" \
		APPLICATION_PID_FILE="$dialog_artifact_dir/application.pid" \
		timeout 360 "$HEADLESS_RUNNER" \
		--socket "big-multimedia-leak-$dialog_key-$$" \
		--log-dir "$dialog_artifact_dir/session" \
		--diagnostic-no-screenshot \
		--wait-ready-query "=$ready_title" \
		--wait-ready-ms 20000 \
		--automation-file "$automation_script" \
		--automation-delay 0 \
		-- bash "$dialog_artifact_dir/launch.sh" env \
		HOME="$dialog_artifact_dir/home" \
		LANG=C.UTF-8 \
		LC_ALL=C \
		BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD=1 \
		XDG_CONFIG_HOME="$dialog_artifact_dir/xdg-config" \
		XDG_CACHE_HOME="$dialog_artifact_dir/xdg-cache" \
		XDG_DATA_HOME="$dialog_artifact_dir/xdg-data" \
		"$binary_path" >"$dialog_artifact_dir/runner.stdout.log" 2>"$dialog_artifact_dir/runner.stderr.log"
	runner_status=$?
	verdict_status=0
	if [ "$runner_status" -eq 0 ]; then
		write_verdict "$samples_path" "$verdict_path"
		verdict_status=$?
	else
		printf 'VERDICT: automation failed before measurement (exit %s)\n' "$runner_status" >"$verdict_path"
		verdict_status="$runner_status"
	fi
	set -e

	cat "$dialog_artifact_dir/runner.stdout.log"
	if [ -s "$dialog_artifact_dir/runner.stderr.log" ]; then
		cat "$dialog_artifact_dir/runner.stderr.log" >&2
	fi
	cat "$verdict_path"
	printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
		"$dialog_artifact_dir" \
		"$dialog_key" \
		"$application_name" \
		"$ready_title" \
		"$atspi_action" \
		"$dialog_title" \
		"$runner_status" \
		"$verdict_status" >>"$SUMMARY_TSV"
	if [ "$verdict_status" -ne 0 ]; then
		overall_status="$verdict_status"
	fi
done

python3 - "$EVIDENCE_DIR/leak-perf.json" "$SUMMARY_TSV" "$ITERATIONS" "$THRESHOLD_KIB" "$BUILD_PROFILE" <<'PY'
import csv
import json
import sys
from pathlib import Path

report_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
iterations = int(sys.argv[3])
threshold_kib = int(sys.argv[4])
build_profile = sys.argv[5]


def first_matching_line(path: Path, needle: str) -> str | None:
    try:
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            if needle in line:
                return line
    except OSError:
        return None
    return None


runs = []
with summary_path.open(encoding="utf-8", newline="") as summary_file:
    reader = csv.reader(summary_file, delimiter="\t")
    for (
        artifact_dir,
        dialog_key,
        application_name,
        ready_title,
        atspi_action,
        dialog_title,
        runner_exit_text,
        verdict_exit_text,
    ) in reader:
        artifact_path = Path(artifact_dir)
        result_path = artifact_path / "probe-result.txt"
        runs.append(
            {
                "dialog": dialog_key,
                "application": application_name,
                "ready_title": ready_title,
                "atspi_action": atspi_action,
                "window_title": dialog_title,
                "runner_exit_code": int(runner_exit_text),
                "exit_code": int(verdict_exit_text),
                "artifact_dir": artifact_dir,
                "samples_csv": str(artifact_path / "samples.csv"),
                "probe_result": str(result_path),
                "verdict": first_matching_line(result_path, "VERDICT:"),
                "slope": first_matching_line(result_path, "per-iteration slope"),
                "automation_log": str(artifact_path / "session" / "automation.log"),
            }
        )

report = {
    "tool": "big-multimedia-dialog-leak-perf",
    "target": "big-multimedia",
    "build_profile": build_profile,
    "iterations": iterations,
    "threshold_kib_per_open": threshold_kib,
    "activation": "AT-SPI action",
    "session": "KWin + AT-SPI",
    "diagnostic_no_screenshot": True,
    "would_fail": any(run["exit_code"] != 0 for run in runs),
    "runs": runs,
}
report_path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
PY

exit "$overall_status"
