#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Regenerate big-media-runtime.pot from every Rust source that shares the
# single `big-media-runtime` gettext domain, and POTFILES.in from the files
# xgettext took strings from.
#
# One catalog translates the whole suite: apps set `big-media-runtime` as the
# process default domain (big_media_runtime::app::boot_player), so `i18n()`/`ni18n()`
# and the components' `text()` translator resolve through it, and
# big-relm4-components' `t()` binds the same domain explicitly.
#
# Usage: ./po/regen-pot.sh   (run from anywhere; paths resolve to the workspace)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"

cd "$ROOT"

# Source trees that own user-visible strings in the shared domain.
SOURCES=(
  crates/media-runtime/src
  crates/big-video-player/src
  crates/big-audio-player/src
  crates/ui/generic/big-relm4-components/src
  crates/ui/media/big-media-components/src
  crates/ui/media/big-player-ui/src
)

# Fail loudly if a tree is missing rather than silently dropping its strings.
for dir in "${SOURCES[@]}"; do
  if [[ ! -d "$dir" ]]; then
    echo "error: missing source tree '$dir' — run from the big-multimedia workspace" >&2
    exit 1
  fi
done

# The C collation keeps the file order, and so the POT, the same on every machine.
candidates="$(mktemp)"
trap 'rm -f "$candidates"' EXIT
find "${SOURCES[@]}" -name '*.rs' -not -path '*/target/*' | LC_ALL=C sort > "$candidates"

xgettext \
  --language=Rust \
  --keyword=i18n \
  --keyword=ni18n:1,2 \
  --keyword=pi18n:1c,2 \
  --keyword=text \
  --keyword=t \
  --keyword=gettext_noop \
  --from-code=UTF-8 \
  --package-name=big-media-runtime \
  --package-version=0.1.0 \
  --copyright-holder="BigLinux" \
  --msgid-bugs-address="https://github.com/biglinux/big-multimedia/issues" \
  --output="$HERE/big-media-runtime.pot" \
  --directory="$ROOT" \
  --files-from="$candidates"

awk '/^#: / { for (i = 2; i <= NF; i++) { sub(/:[0-9]+$/, "", $i); print $i } }' \
  "$HERE/big-media-runtime.pot" | LC_ALL=C sort -u > "$HERE/POTFILES.in"

echo "POT regenerated: $HERE/big-media-runtime.pot"
echo "Files: $(wc -l < "$HERE/POTFILES.in")  Unique msgids: $(grep -c '^msgid ' "$HERE/big-media-runtime.pot")"
