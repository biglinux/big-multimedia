#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
tools="${BIGAGENTS_TOOLS:-}"

if [[ -z "$tools" ]]; then
  for candidate in \
    "$repo_root/../bigagents-tools" \
    "$repo_root/../../pamac-improves/bigagents-tools" \
    "$HOME/pamac-improves/bigagents-tools"; do
    if [[ -f "$candidate/scripts/init.mjs" ]]; then
      tools="$candidate"
      break
    fi
  done
fi

if [[ -z "$tools" ]]; then
  tools="$repo_root/.cache/bigagents-tools"
  mkdir -p "$repo_root/.cache"
  if [[ -d "$tools/.git" ]]; then
    git -C "$tools" pull --ff-only
  else
    git clone --depth=1 https://github.com/biglinux/bigagents-tools "$tools"
  fi
fi

node "$tools/scripts/init.mjs" "$repo_root" --force
node "$tools/scripts/analyze.mjs" "$repo_root" --force
node "$tools/scripts/readiness.mjs" "$repo_root" --bootstrap
