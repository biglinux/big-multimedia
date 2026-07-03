#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
controls_bar="$repo_root/src/video/ui/video_controls_bar.rs"

if ! grep -Fq 'big_player_ui::cut_timeline' "$controls_bar"; then
	printf 'video controls must use big_player_ui::cut_timeline\n' >&2
	exit 1
fi

matches="$(
	grep -nE '\bfn install_cut_timeline_draw\b|\bfn wire_cut_timeline_interaction\b|\bfn hit_handle\b|\bfn hit_body\b|\bfn rounded_rect\b|\benum CutDrag\b|EDGE_GRAB_PX' "$controls_bar" || true
)"
if [ -n "$matches" ]; then
	printf '%s\n' "$matches" >&2
	printf 'cut timeline behavior must stay canonical in big_player_ui::cut_timeline\n' >&2
	exit 1
fi
