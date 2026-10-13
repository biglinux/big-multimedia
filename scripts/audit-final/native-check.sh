#!/usr/bin/env bash
# Maintainer-run gate. Installs no tools, never runs rustup or formats files.
# Run it in an isolated dev session.
set -euo pipefail
export RUSTUP_AUTO_INSTALL=0
root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
logs=${1:?Pass a NEW log-directory path outside the source}
for tool in cargo rustc pkg-config msgfmt; do
  command -v "$tool" >/dev/null || { printf 'BLOCKED: missing %s\n' "$tool" >&2; exit 77; }
done
# Shims must have the configured toolchain already installed. No rustup install here.
if ! rustc -V >/dev/null 2>&1 || ! cargo -V >/dev/null 2>&1; then
  echo 'BLOCKED: required toolchain is not already usable' >&2; exit 77
fi
pkg-config --atleast-version=4.22 gtk4 || { echo 'BLOCKED: GTK 4.22'; exit 77; }
pkg-config --atleast-version=1.9 libadwaita-1 || { echo 'BLOCKED: libadwaita 1.9'; exit 77; }
pkg-config --exists mpv || { echo 'BLOCKED: libmpv development package'; exit 77; }
mkdir -- "$logs"
logs=$(cd -- "$logs" && pwd)
export CARGO_HOME="${CARGO_HOME:-${HOME:?HOME is required}/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
test_home=$(mktemp -d)
trap 'rm -rf -- "$test_home"' EXIT
export HOME="$test_home/home" XDG_CONFIG_HOME="$test_home/config" XDG_DATA_HOME="$test_home/data"
export XDG_CACHE_HOME="$test_home/cache" TMPDIR="$test_home/tmp"
mkdir -p -- "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$TMPDIR"
# HOME isolation is not a filesystem, network, D-Bus or native-code sandbox.
cd -- "$root"
{ rustc -Vv; cargo -V; pkg-config --modversion gtk4 libadwaita-1 mpv; } > "$logs/environment.txt"
failed=0
run_gate() {
  local name=$1; shift
  if "$@" > "$logs/$name.log" 2>&1; then printf 'PASS %s\n' "$name"
  else local status=$?; printf 'FAIL %s exit=%s\n' "$name" "$status"; failed=1; fi
}
run_gate fmt cargo fmt --all -- --check
run_gate check cargo check --locked --workspace --all-targets
run_gate unit cargo test --locked --workspace --lib
run_gate clippy cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
run_gate full-features cargo check --locked --workspace --all-features --all-targets
run_gate minimal cargo check --locked --workspace --no-default-features
# The workspace steps above unify Cargo features and build every target, so a
# player that fails to compile in the configuration its package ships
# still passes them. These two mirror the PKGBUILDs instead.
run_gate audio-package cargo check --locked -p big-audio-player --all-targets
run_gate video-package cargo check --locked -p big-video-player --all-targets
run_gate translations msgfmt --check --check-format -o /dev/null crates/media-runtime/po/pt_BR.po
# Repeat against pristine baseline with identical tools to separate inherited failures.
# Integration/UI tests require controlled fixtures, display and D-Bus; see guide.
exit "$failed"
