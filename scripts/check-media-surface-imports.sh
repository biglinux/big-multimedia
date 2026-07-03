#!/usr/bin/env bash
# Keep media apps behind the canonical big-media-components surface facade.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FORBIDDEN='big_relm4_components::layout::media_window'
FORBIDDEN_FULLSCREEN='set_fullscreened'

matches="$(
	grep -RIn --include='*.rs' "$FORBIDDEN" "$ROOT/crates" || true
)"
fullscreen_matches="$(
	grep -RIn --include='*.rs' "$FORBIDDEN_FULLSCREEN" "$ROOT/crates" || true
)"

if [[ -n "$matches" ]]; then
	printf 'direct media_window imports are forbidden; use big_media_components::surface\n' >&2
	printf '%s\n' "$matches" >&2
	exit 1
fi

if [[ -n "$fullscreen_matches" ]]; then
	printf 'direct fullscreen mutation is forbidden; use big_media_components::surface helpers\n' >&2
	printf '%s\n' "$fullscreen_matches" >&2
	exit 1
fi
