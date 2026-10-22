#!/usr/bin/env bash
# Build the web release and add the service worker, manifest and icons.
#
#   ./scripts/web-build.sh
#   NBX_BUILD_ID="$(git rev-parse HEAD)" ./scripts/web-build.sh
#
# CI and the deploy steps in README use this so a release never ships without
# the PWA files. `dx` leaves old hashed assets behind, so the output directory
# starts empty.
set -euo pipefail

# The bootstrap uses BASH_ENV to repair /tmp; do not recurse through it while
# this script starts child shells for the build.
unset BASH_ENV

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target="${CARGO_TARGET_DIR:-$root/target}"

ensure_clang() {
  command -v clang >/dev/null 2>&1 && return 0
  [ -n "${CC_wasm32_unknown_unknown:-}" ] && return 0
  [ "${NOTEBOX_SKIP_CLANG_INSTALL:-0}" = 1 ] && return 1

  local apt=(env DEBIAN_FRONTEND=noninteractive apt-get)
  if [ "$(id -u)" -ne 0 ]; then
    command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null || return 1
    apt=(sudo -n "${apt[@]}")
  fi

  echo "web-build: installing clang (needed by the bundled SQLite wasm build)" >&2
  if ! timeout 300 "${apt[@]}" install -y clang >/dev/null 2>&1; then
    timeout 300 "${apt[@]}" update >/dev/null 2>&1 || return 1
    timeout 300 "${apt[@]}" install -y clang >/dev/null 2>&1 || return 1
  fi
  command -v clang >/dev/null 2>&1
}

if ! ensure_clang; then
  echo "web-build: clang is required for the bundled SQLite wasm build" >&2
  echo "web-build: install it with: sudo apt-get install -y clang" >&2
  echo "web-build: or set CC_wasm32_unknown_unknown to a C compiler" >&2
  exit 1
fi
rm -rf "$target/dx/notebox/release/web/public"
dx build -p notes-app --platform web --release --features web
node scripts/pwa-build.mjs "$target/dx/notebox/release/web/public"
