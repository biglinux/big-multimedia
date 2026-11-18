#!/usr/bin/env bash
# Every subprocess passes an argv array: a shell command string would let
# file names and URLs from outside be parsed as shell syntax.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

mapfile -t sources < <(find "$ROOT/crates" -type d -name src -prune)
if ((${#sources[@]} == 0)); then
	printf 'no crate src/ directories under %s\n' "$ROOT/crates" >&2
	exit 1
fi

status=0
matches="$(
	grep -RIn --include='*.rs' -F \
		-e '"sh", "-c"' \
		-e '"bash", "-c"' \
		-e 'Command::new("sh")' \
		-e 'Command::new("bash")' \
		"${sources[@]}"
)" || status=$?

if ((status > 1)); then
	exit "$status"
fi

if [[ -n "$matches" ]]; then
	printf 'shell invocations are forbidden; pass an argv array\n' >&2
	printf '%s\n' "$matches" >&2
	exit 1
fi
