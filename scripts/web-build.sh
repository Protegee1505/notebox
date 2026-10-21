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

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

target="${CARGO_TARGET_DIR:-$root/target}"
rm -rf "$target/dx/notebox/release/web/public"
dx build -p notes-app --platform web --release --features web
node scripts/pwa-build.mjs "$target/dx/notebox/release/web/public"
