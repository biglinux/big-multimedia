#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail

MODE="${1:---ci}"
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

run() {
	printf '\n==> %s\n' "$1"
	shift
	"$@"
}

require_tool() {
	if ! command -v "$1" >/dev/null 2>&1; then
		printf 'missing required tool: %s\n' "$1" >&2
		exit 127
	fi
}

require_cargo_tool() {
	if ! cargo "$1" --version >/dev/null 2>&1; then
		printf 'missing required cargo tool: cargo-%s\n' "$1" >&2
		exit 127
	fi
}

find_bigagents_tools() {
	if [[ -n "${BIGAGENTS_TOOLS:-}" && -f "${BIGAGENTS_TOOLS}/scripts/native-deps.mjs" ]]; then
		printf '%s\n' "$BIGAGENTS_TOOLS"
		return
	fi
	for candidate in \
		"$HOME/.agents" \
		"$repo_root/../bigagents-tools" \
		"$repo_root/../../pamac-improves/bigagents-tools" \
		"$HOME/pamac-improves/bigagents-tools"; do
		if [[ -f "$candidate/scripts/native-deps.mjs" ]]; then
			printf '%s\n' "$candidate"
			return
		fi
	done
	printf 'missing bigagents-tools checkout; run dev/update-bigagents.sh or set BIGAGENTS_TOOLS\n' >&2
	exit 127
}

cd "$repo_root"

require_tool cargo
require_tool python3
require_tool node
require_tool gitleaks
require_tool trivy

if [[ "$MODE" == "--fix" ]]; then
	run rustfmt cargo fmt --all
	exit 0
fi

if [[ "$MODE" != "--ci" && "$MODE" != "--full" && "$MODE" != "--deep" ]]; then
	printf 'usage: %s [--ci|--fix|--full|--deep]\n' "$0" >&2
	exit 2
fi

run rustfmt cargo fmt --all -- --check
run video-render-contract python3 dev/render-path-contract.py
run clippy cargo clippy --all-targets --all-features --locked -- -D warnings
run tests cargo test --all-targets --all-features --locked
mkdir -p target/.doctest-tmp
run doctests env TMPDIR="$PWD/target/.doctest-tmp" cargo test --doc --all-features --locked
run rustdoc env RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --locked
run native-deps node "$(find_bigagents_tools)/scripts/native-deps.mjs" .
run component-check node "$(find_bigagents_tools)/scripts/component-check.mjs" . --strict
run domain-policy node "$(find_bigagents_tools)/scripts/biglinux-domain-policy.mjs" --biglinux .

if [[ -d .git ]] && git rev-parse --verify HEAD >/dev/null 2>&1; then
	run gitleaks gitleaks git . --no-banner --redact
else
	run gitleaks gitleaks dir . --no-banner --redact
fi
run trivy trivy fs --skip-dirs target --skip-dirs build --skip-dirs builddir --skip-dirs node_modules --skip-dirs .flatpak-builder .

if [[ "$MODE" == "--full" || "$MODE" == "--deep" ]]; then
	if command -v cargo-auditable >/dev/null 2>&1; then
		run auditable cargo auditable build --release --locked
	fi
fi

if [[ "$MODE" == "--deep" ]]; then
	if cargo hack --version >/dev/null 2>&1; then
		run feature-matrix cargo hack check --workspace --feature-powerset --no-dev-deps
	fi
	if cargo semver-checks --version >/dev/null 2>&1; then
		run semver cargo semver-checks check-release
	fi
	if cargo mutants --version >/dev/null 2>&1; then
		run mutants cargo mutants
	fi
	if cargo +nightly miri --version >/dev/null 2>&1; then
		run miri cargo +nightly miri test
	fi
fi
