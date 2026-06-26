#!/usr/bin/env bash
set -euo pipefail

SCRIPT_NAME="${0##*/}"
OUT_DIR="${OUT_DIR:-/tmp/big-multimedia-ui-smoke}"
RUN_AUDIO=0
RUN_VIDEO=0
KEEP_RUNNING=0

resolve_aigui() {
	local candidate
	if [ -n "${AIGUI:-}" ]; then
		printf '%s\n' "$AIGUI"
		return
	fi
	if candidate="$(command -v aigui 2>/dev/null)"; then
		printf '%s\n' "$candidate"
		return
	fi
	if [ -n "${AGENTS_HOME:-}" ]; then
		candidate="$AGENTS_HOME/skills/linux-ui-a11y/scripts/aigui"
		if [ -x "$candidate" ]; then
			printf '%s\n' "$candidate"
			return
		fi
	fi
	if [ -n "${HOME:-}" ]; then
		for candidate in \
			"$HOME/.agents/skills/linux-ui-a11y/scripts/aigui" \
			"$HOME/.codex/skills/linux-ui-a11y/scripts/aigui"; do
			if [ -x "$candidate" ]; then
				printf '%s\n' "$candidate"
				return
			fi
		done
	fi
}

AIGUI="$(resolve_aigui)"

usage() {
	cat <<EOF
Usage: $SCRIPT_NAME [--audio] [--video] [--all] [--keep-running]

Drives the installed big-host media modules through aigui and captures menu
screenshots. Set AIGUI_HOST/AIGUI_PASS when driving the VM.

Environment:
  AIGUI     Path to the aigui script. Auto-detected from PATH, AGENTS_HOME, or HOME.
  OUT_DIR   Local screenshot directory. Default: $OUT_DIR
EOF
}

fail() {
	printf '%s: %s\n' "$SCRIPT_NAME" "$*" >&2
	exit 1
}

note() {
	printf '%s\n' "$*" >&2
}

run_aigui() {
	"$AIGUI" "$@"
}

wait_for_control() {
	local query="$1"
	local role="$2"
	local timeout_ms="$3"
	local elapsed_ms=0
	local step_ms=500

	while [ "$elapsed_ms" -le "$timeout_ms" ]; do
		if run_aigui find "$query" "$role" >/dev/null 2>&1; then
			return 0
		fi
		sleep 0.5
		elapsed_ms=$((elapsed_ms + step_ms))
	done

	fail "timed out waiting for $query ($role)"
}

capture_shot() {
	local name="$1"
	local path="$OUT_DIR/$name.png"
	mkdir -p "$OUT_DIR"
	run_aigui --out "$path" shot >/dev/null
	assert_1280x720 "$path"
	printf '%s\n' "$path"
}

assert_1280x720() {
	local path="$1"
	local description
	description="$(file "$path")"
	case "$description" in
	*"1280 x 720"*) ;;
	*) fail "expected 1280x720 screenshot, got: $description" ;;
	esac
}

open_menu_and_capture() {
	local unit="$1"
	local query="$2"
	local role="$3"
	local shot_name="$4"

	note "Finding $query ($role)"
	run_aigui find "$query" "$role" >/dev/null
	run_aigui act "$query" "$role" >/dev/null
	sleep 0.4
	local shot
	shot="$(capture_shot "$shot_name")"
	note "Captured $shot"
	run_aigui key esc >/dev/null || true
}

launch_module() {
	local unit="$1"
	local module="$2"

	run_aigui stop "$unit" >/dev/null 2>&1 || true
	run_aigui launch "$unit" -- /usr/bin/big-host --separate "$module" >/dev/null
	wait_for_control "=Main Menu" button 15000
}

stop_module() {
	local unit="$1"
	if [ "$KEEP_RUNNING" -eq 0 ]; then
		run_aigui stop "$unit" >/dev/null 2>&1 || true
	fi
}

smoke_audio() {
	local unit="big-multimedia-menu-audio"
	note "Launching audio player"
	launch_module "$unit" player-audio
	open_menu_and_capture "$unit" "=Main Menu" button audio-main-menu
	stop_module "$unit"
}

smoke_video() {
	local unit="big-multimedia-menu-video"
	note "Launching video player"
	launch_module "$unit" player-video
	open_menu_and_capture "$unit" "=Main Menu" button video-main-menu
	open_menu_and_capture "$unit" "=Open" button video-open-menu
	stop_module "$unit"
}

while [ "$#" -gt 0 ]; do
	case "$1" in
	--audio)
		RUN_AUDIO=1
		shift
		;;
	--video)
		RUN_VIDEO=1
		shift
		;;
	--all)
		RUN_AUDIO=1
		RUN_VIDEO=1
		shift
		;;
	--keep-running)
		KEEP_RUNNING=1
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*) fail "unknown argument: $1" ;;
	esac
done

[ -x "$AIGUI" ] || fail "aigui not executable: $AIGUI"
if [ "$RUN_AUDIO" -eq 0 ] && [ "$RUN_VIDEO" -eq 0 ]; then
	RUN_AUDIO=1
	RUN_VIDEO=1
fi

if [ "$RUN_AUDIO" -eq 1 ]; then
	smoke_audio
fi
if [ "$RUN_VIDEO" -eq 1 ]; then
	smoke_video
fi

note "Menu smoke complete. Screenshots: $OUT_DIR"
